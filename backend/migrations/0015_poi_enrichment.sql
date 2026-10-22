-- Enrichissement des lieux : notes, liens et photos, ajoutés par les membres ou par
-- l'assistant (`via_assistant`). Après suppression d'un compte, ils restent sans auteur,
-- comme les lieux.

CREATE TABLE poi_notes (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    poi_id        uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    author_id     uuid REFERENCES users(id) ON DELETE SET NULL,
    via_assistant boolean NOT NULL DEFAULT false,
    body          text NOT NULL CHECK (length(body) BETWEEN 1 AND 4000),
    -- Page d'où vient l'information (http(s)).
    source_url    text CHECK (length(source_url) <= 500),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER poi_notes_updated_at BEFORE UPDATE ON poi_notes
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX poi_notes_poi_id_idx ON poi_notes(poi_id, created_at);
CREATE INDEX poi_notes_author_id_idx ON poi_notes(author_id);

CREATE TABLE poi_links (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    poi_id        uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    author_id     uuid REFERENCES users(id) ON DELETE SET NULL,
    via_assistant boolean NOT NULL DEFAULT false,
    url           text NOT NULL CHECK (url LIKE 'https://%' AND length(url) <= 500),
    title         text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    kind          text NOT NULL CHECK (kind IN
                      ('official', 'booking', 'article', 'video', 'map', 'other')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT poi_links_poi_url_unique UNIQUE (poi_id, url)
);
CREATE INDEX poi_links_author_id_idx ON poi_links(author_id);

-- Les fichiers (image et vignette, ré-encodées sans métadonnées) sont dans le stockage de
-- photos, sous l'identifiant de la ligne.
CREATE TABLE poi_photos (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    poi_id        uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    author_id     uuid REFERENCES users(id) ON DELETE SET NULL,
    via_assistant boolean NOT NULL DEFAULT false,
    width         integer NOT NULL CHECK (width > 0),
    height        integer NOT NULL CHECK (height > 0),
    mime          text NOT NULL CHECK (mime IN ('image/webp', 'image/jpeg')),
    bytes         integer NOT NULL CHECK (bytes > 0),
    -- Image d'origine et page qui la présente (photos importées depuis le web).
    source_url    text CHECK (length(source_url) <= 500),
    source_page   text CHECK (length(source_page) <= 500),
    -- Crédit et licence affichés avec la photo.
    author_credit text CHECK (length(author_credit) <= 200),
    license       text CHECK (length(license) <= 100),
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX poi_photos_poi_id_idx ON poi_photos(poi_id, created_at);
CREATE INDEX poi_photos_author_id_idx ON poi_photos(author_id);

-- Le champ unique `pois.notes` devient la première note de chaque lieu.
INSERT INTO poi_notes (poi_id, author_id, body, created_at, updated_at)
SELECT id, created_by, notes, created_at, updated_at FROM pois WHERE notes <> '';
ALTER TABLE pois DROP COLUMN notes;
