//! Photos des lieux : traitement et stockage.
//!
//! Toute image reçue (envoi d'un membre ou téléchargement par l'assistant) est décodée avec
//! des limites (dimensions, mémoire), redressée selon son orientation EXIF puis ré-encodée
//! en JPEG à partir des seuls pixels : aucune métadonnée d'origine (position GPS, appareil)
//! n'est conservée. Une vignette accompagne chaque photo.
//!
//! Les fichiers sont derrière le trait [`PhotoStore`] : dossier local ([`LocalPhotoStore`])
//! en dev, stockage compatible S3 plus tard ; [`MemoryPhotoStore`] pour les tests.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::Context;
use async_trait::async_trait;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbImage};
use sqlx::PgPool;
use uuid::Uuid;

use crate::safe_fetch::WebFetcher;

/// Taille maximale d'une image reçue ou téléchargée.
pub const MAX_INPUT_BYTES: usize = 10_000_000;
/// Nombre maximal de photos par lieu.
pub const MAX_PER_POI: i64 = 20;
/// Plus grand côté de la photo enregistrée, puis de sa vignette.
const FULL_SIZE: u32 = 1600;
const THUMB_SIZE: u32 = 400;
const JPEG_QUALITY: u8 = 82;
/// Limites de décodage (une petite image compressée peut annoncer des dimensions énormes).
const MAX_DIMENSION: u32 = 12_000;
const MAX_DECODE_ALLOC: u64 = 300 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PhotoError {
    #[error("format d'image non pris en charge (JPEG, PNG, WebP ou GIF)")]
    Unsupported,
    #[error("image illisible ou trop grande")]
    Invalid(#[source] image::ImageError),
}

/// Photo prête à être stockée.
pub struct Processed {
    pub full: Vec<u8>,
    pub thumb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Décode, redresse et ré-encode une image (opération coûteuse : à lancer hors du
/// fil asynchrone, avec `spawn_blocking`).
pub fn process(bytes: &[u8]) -> Result<Processed, PhotoError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| PhotoError::Unsupported)?;
    // Le format est reconnu au contenu, jamais d'après le nom ou le type annoncé.
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif)
    ) {
        return Err(PhotoError::Unsupported);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().map_err(PhotoError::Invalid)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(PhotoError::Invalid)?;
    img.apply_orientation(orientation);

    let full = flatten(fit(&img, FULL_SIZE, FilterType::Lanczos3));
    let thumb = flatten(fit(&img, THUMB_SIZE, FilterType::Triangle));
    Ok(Processed {
        width: full.width(),
        height: full.height(),
        full: jpeg(&full)?,
        thumb: jpeg(&thumb)?,
    })
}

/// Réduit l'image pour que son plus grand côté tienne dans `size` (jamais agrandie).
fn fit(img: &DynamicImage, size: u32, filter: FilterType) -> DynamicImage {
    if img.width() <= size && img.height() <= size {
        img.clone()
    } else {
        img.resize(size, size, filter)
    }
}

/// Pixels RGB ; la transparence est posée sur fond blanc (JPEG n'en a pas).
fn flatten(img: DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.into_rgb8();
    }
    let rgba = img.into_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let over_white =
            |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
        image::Rgb([over_white(r), over_white(g), over_white(b)])
    })
}

fn jpeg(img: &RgbImage) -> Result<Vec<u8>, PhotoError> {
    let mut out = Vec::new();
    img.write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
        .map_err(PhotoError::Invalid)?;
    Ok(out)
}

/// Clés des fichiers d'une photo.
pub fn full_key(id: Uuid) -> String {
    format!("{id}.jpg")
}

pub fn thumb_key(id: Uuid) -> String {
    format!("{id}-thumb.jpg")
}

/// Identifiant de photo d'une clé (`<uuid>.jpg` ou `<uuid>-thumb.jpg`).
fn photo_id(key: &str) -> Option<Uuid> {
    let stem = key.strip_suffix(".jpg")?;
    stem.strip_suffix("-thumb").unwrap_or(stem).parse().ok()
}

#[async_trait]
pub trait PhotoStore: Send + Sync {
    async fn put(&self, key: &str, bytes: Vec<u8>) -> anyhow::Result<()>;
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>>;
    /// Sans effet si le fichier n'existe pas.
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
    /// Toutes les clés, avec leur date d'écriture.
    async fn list(&self) -> anyhow::Result<Vec<(String, SystemTime)>>;
}

/// Photos dans un dossier local.
pub struct LocalPhotoStore {
    dir: PathBuf,
}

impl LocalPhotoStore {
    pub fn new(dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("dossier des photos {}", dir.display()))?;
        Ok(Self { dir })
    }

    fn path(&self, key: &str) -> anyhow::Result<PathBuf> {
        // Clés produites par ce module uniquement : jamais de chemin arbitraire.
        anyhow::ensure!(photo_id(key).is_some(), "clé de photo invalide : {key}");
        Ok(self.dir.join(key))
    }
}

#[async_trait]
impl PhotoStore for LocalPhotoStore {
    async fn put(&self, key: &str, bytes: Vec<u8>) -> anyhow::Result<()> {
        let path = self.path(key)?;
        // Écriture puis renommage : un fichier n'est jamais lu à moitié écrit.
        let tmp = path.with_extension("tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        match tokio::fs::read(self.path(key)?).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        match tokio::fs::remove_file(self.path(key)?).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    async fn list(&self) -> anyhow::Result<Vec<(String, SystemTime)>> {
        let mut out = vec![];
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let Ok(key) = entry.file_name().into_string() else {
                continue;
            };
            if photo_id(&key).is_some() {
                out.push((key, entry.metadata().await?.modified()?));
            }
        }
        Ok(out)
    }
}

/// Photos en mémoire (tests).
#[derive(Default)]
pub struct MemoryPhotoStore {
    files: Mutex<HashMap<String, (Vec<u8>, SystemTime)>>,
}

impl MemoryPhotoStore {
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self
            .files
            .lock()
            .expect("mutex empoisonné")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// Vieillit tous les fichiers (tests du nettoyage).
    pub fn age_all(&self, by: Duration) {
        for (_, at) in self.files.lock().expect("mutex empoisonné").values_mut() {
            *at -= by;
        }
    }
}

#[async_trait]
impl PhotoStore for Arc<MemoryPhotoStore> {
    async fn put(&self, key: &str, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.files
            .lock()
            .expect("mutex empoisonné")
            .insert(key.to_owned(), (bytes, SystemTime::now()));
        Ok(())
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self
            .files
            .lock()
            .expect("mutex empoisonné")
            .get(key)
            .map(|(b, _)| b.clone()))
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.files.lock().expect("mutex empoisonné").remove(key);
        Ok(())
    }

    async fn list(&self) -> anyhow::Result<Vec<(String, SystemTime)>> {
        Ok(self
            .files
            .lock()
            .expect("mutex empoisonné")
            .iter()
            .map(|(k, (_, at))| (k.clone(), *at))
            .collect())
    }
}

/// Stockage des photos et téléchargement des images importées depuis le web.
pub struct PhotoService {
    pub store: Arc<dyn PhotoStore>,
    pub web: Arc<dyn WebFetcher>,
}

impl PhotoService {
    pub fn new(store: impl PhotoStore + 'static, web: impl WebFetcher + 'static) -> Self {
        Self {
            store: Arc::new(store),
            web: Arc::new(web),
        }
    }
}

/// Fichiers plus récents que ce délai : peut-être une photo en cours d'enregistrement.
const SWEEP_GRACE: Duration = Duration::from_secs(15 * 60);

/// Supprime les fichiers dont la photo n'existe plus (lieu, voyage ou compte supprimés :
/// les lignes partent en cascade, les fichiers restent). Renvoie le nombre de fichiers
/// supprimés.
pub async fn sweep_orphans(db: &PgPool, store: &dyn PhotoStore) -> anyhow::Result<usize> {
    let now = SystemTime::now();
    let candidates: Vec<(String, Uuid)> = store
        .list()
        .await?
        .into_iter()
        .filter(|(_, at)| now.duration_since(*at).is_ok_and(|age| age > SWEEP_GRACE))
        .filter_map(|(key, _)| photo_id(&key).map(|id| (key, id)))
        .collect();
    if candidates.is_empty() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = candidates.iter().map(|(_, id)| *id).collect();
    let alive: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM poi_photos WHERE id = ANY($1)")
        .bind(&ids)
        .fetch_all(db)
        .await?;
    let mut removed = 0;
    for (key, id) in candidates {
        if !alive.contains(&id) {
            store.delete(&key).await?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use image::{ImageEncoder, Rgba, RgbaImage};

    use super::*;

    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba(pixel));
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    #[test]
    fn large_images_are_reduced_and_reencoded_as_jpeg() {
        let p = process(&png(3200, 1600, [10, 120, 60, 255])).unwrap();
        assert_eq!((p.width, p.height), (1600, 800));
        assert_eq!(&p.full[..3], &[0xFF, 0xD8, 0xFF], "JPEG");
        let thumb = image::load_from_memory(&p.thumb).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (400, 200));
    }

    #[test]
    fn small_images_are_not_enlarged() {
        let p = process(&png(300, 200, [0, 0, 0, 255])).unwrap();
        assert_eq!((p.width, p.height), (300, 200));
    }

    #[test]
    fn transparency_becomes_white() {
        let p = process(&png(10, 10, [0, 0, 0, 0])).unwrap();
        let px = image::load_from_memory(&p.full).unwrap().into_rgb8();
        assert!(px.get_pixel(5, 5).0.iter().all(|&c| c > 245));
    }

    #[test]
    fn exif_orientation_is_applied_and_metadata_dropped() {
        // JPEG 2×1 (paysage) avec EXIF Orientation = 6 (rotation de 90°) et un champ GPS.
        let img = RgbImage::from_pixel(2, 1, image::Rgb([200, 30, 30]));
        let mut plain = Vec::new();
        img.write_with_encoder(JpegEncoder::new_with_quality(&mut plain, 90))
            .unwrap();
        let exif: &[u8] = &[
            b'E', b'x', b'i', b'f', 0, 0, // en-tête APP1
            b'M', b'M', 0, 42, 0, 0, 0, 8, // TIFF big-endian, IFD0 à l'octet 8
            0, 2, // 2 entrées
            0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, // Orientation = 6
            0x88, 0x25, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0, // pointeur GPS (valeur factice)
            0, 0, 0, 0,
        ];
        let len = (exif.len() + 2) as u16;
        let mut with_exif = vec![0xFF, 0xD8, 0xFF, 0xE1];
        with_exif.extend_from_slice(&len.to_be_bytes());
        with_exif.extend_from_slice(exif);
        with_exif.extend_from_slice(&plain[2..]);

        let p = process(&with_exif).unwrap();
        assert_eq!((p.width, p.height), (1, 2), "redressée en portrait");
        assert!(
            !p.full.windows(4).any(|w| w == b"Exif"),
            "aucune métadonnée EXIF conservée"
        );
    }

    #[test]
    fn non_images_and_bombs_are_rejected() {
        assert!(matches!(
            process(b"<svg></svg>"),
            Err(PhotoError::Unsupported)
        ));
        assert!(matches!(
            process(b"GIF89a tronque"),
            Err(PhotoError::Invalid(_))
        ));
        // En-tête PNG annonçant 100 000 × 100 000 pixels.
        let mut bomb = png(1, 1, [0, 0, 0, 255]);
        bomb[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(matches!(process(&bomb), Err(PhotoError::Invalid(_))));
    }

    #[test]
    fn keys_map_back_to_photo_ids() {
        let id = Uuid::new_v4();
        assert_eq!(photo_id(&full_key(id)), Some(id));
        assert_eq!(photo_id(&thumb_key(id)), Some(id));
        assert_eq!(photo_id("../etc/passwd"), None);
        assert_eq!(photo_id(&format!("{id}.png")), None);
    }

    #[tokio::test]
    async fn local_store_round_trip() {
        let dir = std::env::temp_dir().join(format!("tg-photos-{}", Uuid::new_v4()));
        let store = LocalPhotoStore::new(&dir).unwrap();
        let key = full_key(Uuid::new_v4());
        store.put(&key, b"jpeg".to_vec()).await.unwrap();
        assert_eq!(
            store.get(&key).await.unwrap().as_deref(),
            Some(&b"jpeg"[..])
        );
        assert_eq!(store.list().await.unwrap().len(), 1);
        store.delete(&key).await.unwrap();
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
        assert!(store.put("../evil.jpg", vec![]).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
