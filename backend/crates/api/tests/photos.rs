mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use api::assistant::AssistantCall;
use common::{png, TestApp, ORIGIN, WEB_NOT_AN_IMAGE, WEB_PHOTO};

struct Ctx {
    orga: String,
    editor: String,
    viewer: String,
    trip: String,
    poi: String,
}

impl Ctx {
    fn uri(&self) -> String {
        format!("/api/v1/trips/{}/pois/{}/photos", self.trip, self.poi)
    }
}

async fn setup(app: &TestApp) -> Ctx {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &editor, "editor").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    let poi = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": "Volcán Arenal", "category": "nature", "lat": 10.46, "lon": -84.70 }),
            Some(&orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    Ctx {
        orga,
        editor,
        viewer,
        trip,
        poi,
    }
}

const BOUNDARY: &str = "----tg-test-boundary";

/// Corps multipart : un fichier `file` et des champs texte.
fn multipart(file: Option<&[u8]>, fields: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    if let Some(file) = file {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\r\n\
                 Content-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(file);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn upload(app: &TestApp, uri: &str, body: Vec<u8>, session: &str) -> common::Response {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("tg_session={session}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    app.send(req).await
}

/// Fichier servi : statut, en-têtes utiles et contenu brut.
async fn fetch_file(
    app: &TestApp,
    uri: &str,
    session: &str,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let req = Request::builder()
        .uri(uri)
        .header(header::COOKIE, format!("tg_session={session}"))
        .body(Body::empty())
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let nosniff = res
        .headers()
        .get(header::X_CONTENT_TYPE_OPTIONS)
        .map(|v| v.to_str().unwrap().to_owned());
    if status == StatusCode::OK {
        assert_eq!(res.headers()[header::CONTENT_TYPE], "image/jpeg");
    }
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, nosniff, bytes)
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn uploaded_photo_is_resized_and_served_to_members_only(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    let outsider = app.register("autre@example.com", "Autre").await;

    let res = upload(
        &app,
        &c.uri(),
        multipart(
            Some(&png(2000, 1000)),
            &[("author_credit", "Lucía"), ("license", "CC BY 4.0")],
        ),
        &c.editor,
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(
        (res.body["width"].as_i64(), res.body["height"].as_i64()),
        (Some(1600), Some(800))
    );
    assert_eq!(res.body["author_credit"], "Lucía");
    assert_eq!(res.body["license"], "CC BY 4.0");
    assert_eq!(res.body["author_name"], "Éditrice");
    assert_eq!(res.body["via_assistant"], false);
    assert_eq!(app.photo_store.keys().len(), 2, "image et vignette");

    let image_url = res.body["image_url"].as_str().unwrap().to_owned();
    let thumb_url = res.body["thumb_url"].as_str().unwrap().to_owned();
    let (status, nosniff, bytes) = fetch_file(&app, &image_url, &c.viewer).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(nosniff.as_deref(), Some("nosniff"));
    assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 1600);
    let (status, _, thumb) = fetch_file(&app, &thumb_url, &c.viewer).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(image::load_from_memory(&thumb).unwrap().width(), 400);

    assert_eq!(
        fetch_file(&app, &image_url, &outsider).await.0,
        StatusCode::NOT_FOUND
    );
    // Photo d'un autre lieu : introuvable sous celui-ci.
    let other_poi = app
        .post(
            &format!("/api/v1/trips/{}/pois", c.trip),
            json!({ "name": "Tabacón", "category": "nature", "lat": 10.49, "lon": -84.72 }),
            Some(&c.orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let wrong = image_url.replace(&c.poi, &other_poi);
    assert_eq!(
        fetch_file(&app, &wrong, &c.orga).await.0,
        StatusCode::NOT_FOUND
    );

    let list = app.get(&c.uri(), Some(&c.viewer)).await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(list.body[0]["image_url"], image_url.as_str());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invalid_uploads_are_rejected_without_storing_anything(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    for (body, what) in [
        (multipart(None, &[]), "sans fichier"),
        (
            multipart(Some(b"<svg onload=alert(1)>"), &[]),
            "pas une image",
        ),
        (
            multipart(Some(&png(10, 10)), &[("author_credit", "deux\nlignes")]),
            "crédit",
        ),
        (
            multipart(Some(&png(10, 10)), &[("source_page", "javascript:x")]),
            "source",
        ),
    ] {
        let res = upload(&app, &c.uri(), body, &c.editor).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{what} : {:?}",
            res.body
        );
    }
    assert!(app.photo_store.keys().is_empty());

    // Plus de 10 Mo : refusé avant même d'être lu en entier.
    let huge = upload(
        &app,
        &c.uri(),
        multipart(Some(&vec![0u8; 11_000_000]), &[]),
        &c.editor,
    )
    .await;
    assert_eq!(huge.status, StatusCode::BAD_REQUEST);
    assert!(
        huge.body["message"].as_str().unwrap().contains("10 Mo"),
        "{:?}",
        huge.body
    );

    let viewer = upload(
        &app,
        &c.uri(),
        multipart(Some(&png(10, 10)), &[]),
        &c.viewer,
    )
    .await;
    assert_eq!(viewer.status, StatusCode::FORBIDDEN);
    assert!(app.photo_store.keys().is_empty());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn photos_are_imported_from_the_web_with_their_credit(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    let import = format!("{}/import", c.uri());

    let res = app
        .post(
            &import,
            json!({
                "image_url": WEB_PHOTO,
                "source_page": "https://commons.wikimedia.org/wiki/File:Arenal.png",
                "author_credit": "Ana",
                "license": "CC BY-SA 4.0",
            }),
            Some(&c.editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["source_url"], WEB_PHOTO);
    assert_eq!(
        res.body["source_page"],
        "https://commons.wikimedia.org/wiki/File:Arenal.png"
    );
    assert_eq!(res.body["width"], 1600);

    for (url, what) in [
        (WEB_NOT_AN_IMAGE, "page HTML"),
        ("https://example.org/introuvable.jpg", "404"),
        ("http://upload.wikimedia.org/arenal.png", "http"),
        (
            "https://user:pw@upload.wikimedia.org/arenal.png",
            "identifiants",
        ),
    ] {
        let res = app
            .post(&import, json!({ "image_url": url }), Some(&c.editor))
            .await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{what} : {:?}",
            res.body
        );
    }
    // Seule la photo valide est stockée.
    assert_eq!(app.photo_store.keys().len(), 2);

    let viewer = app
        .post(&import, json!({ "image_url": WEB_PHOTO }), Some(&c.viewer))
        .await;
    assert_eq!(viewer.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn assistant_imports_are_attributed(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    let mut req = Request::builder()
        .method(Method::POST)
        .uri(format!("{}/import", c.uri()))
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("tg_session={}", c.editor))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "image_url": WEB_PHOTO }).to_string()))
        .unwrap();
    req.extensions_mut().insert(AssistantCall {
        conversation_id: Uuid::new_v4(),
    });
    let res = app.send(req).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["via_assistant"], true);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn photos_per_place_are_limited(db: PgPool) {
    let app = TestApp::new(db.clone());
    let c = setup(&app).await;
    let poi: Uuid = c.poi.parse().unwrap();
    sqlx::query(
        "INSERT INTO poi_photos (poi_id, width, height, mime, bytes)
         SELECT $1, 10, 10, 'image/jpeg', 100 FROM generate_series(1, 20)",
    )
    .bind(poi)
    .execute(&db)
    .await
    .unwrap();

    let res = upload(
        &app,
        &c.uri(),
        multipart(Some(&png(10, 10)), &[]),
        &c.editor,
    )
    .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body["message"].as_str().unwrap().contains("20"));
    assert!(app.photo_store.keys().is_empty());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn deleting_photos_removes_their_files(db: PgPool) {
    let app = TestApp::new(db.clone());
    let c = setup(&app).await;
    let photo = upload(
        &app,
        &c.uri(),
        multipart(Some(&png(50, 50)), &[]),
        &c.editor,
    )
    .await;
    let photo_uri = format!("{}/{}", c.uri(), photo.body["id"].as_str().unwrap());

    assert_eq!(
        app.delete(&photo_uri, Some(&c.viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.delete(&photo_uri, Some(&c.editor)).await.status,
        StatusCode::NO_CONTENT
    );
    assert!(app.photo_store.keys().is_empty());
    let image_url = photo.body["image_url"].as_str().unwrap();
    assert_eq!(
        fetch_file(&app, image_url, &c.orga).await.0,
        StatusCode::NOT_FOUND
    );

    // Lieu supprimé : la ligne part en cascade, les fichiers restent jusqu'au nettoyage.
    upload(
        &app,
        &c.uri(),
        multipart(Some(&png(50, 50)), &[]),
        &c.editor,
    )
    .await;
    let kept_poi = app
        .post(
            &format!("/api/v1/trips/{}/pois", c.trip),
            json!({ "name": "Tabacón", "category": "nature", "lat": 10.49, "lon": -84.72 }),
            Some(&c.orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let kept = upload(
        &app,
        &format!("/api/v1/trips/{}/pois/{kept_poi}/photos", c.trip),
        multipart(Some(&png(50, 50)), &[]),
        &c.editor,
    )
    .await;
    assert_eq!(kept.status, StatusCode::CREATED);
    app.delete(
        &format!("/api/v1/trips/{}/pois/{}", c.trip, c.poi),
        Some(&c.orga),
    )
    .await;
    assert_eq!(app.photo_store.keys().len(), 4);

    // Fichiers récents : peut-être une photo en cours d'enregistrement, gardés.
    let store = app.photo_store.clone();
    assert_eq!(api::photos::sweep_orphans(&db, &store).await.unwrap(), 0);
    app.photo_store.age_all(Duration::from_secs(3600));
    assert_eq!(api::photos::sweep_orphans(&db, &store).await.unwrap(), 2);
    let kept_id = kept.body["id"].as_str().unwrap();
    assert!(app
        .photo_store
        .keys()
        .iter()
        .all(|k| k.starts_with(kept_id)));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn photos_are_exported_and_kept_after_account_deletion(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    upload(
        &app,
        &c.uri(),
        multipart(Some(&png(50, 50)), &[("author_credit", "Éditrice")]),
        &c.editor,
    )
    .await;

    let export = app.get("/api/v1/me/export", Some(&c.editor)).await;
    assert_eq!(export.body["poi_photos"][0]["poi_name"], "Volcán Arenal");
    assert_eq!(export.body["poi_photos"][0]["author_credit"], "Éditrice");

    let deleted = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "editor@example.com", "password": common::PASSWORD })),
            Some(&c.editor),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let list = app.get(&c.uri(), Some(&c.orga)).await;
    assert_eq!(list.body[0]["author_id"], Value::Null);
}
