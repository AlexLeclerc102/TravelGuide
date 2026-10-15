-- Suggestions de lieux : lieux d'intérêt importés d'un extrait OpenStreetMap (interrogés
-- localement) et suggestions écartées par le groupe.

CREATE TABLE osm_places (
    osm_type      text NOT NULL CHECK (osm_type IN ('node', 'way', 'relation')),
    osm_id        bigint NOT NULL CHECK (osm_id > 0),
    name          text NOT NULL,
    category      text NOT NULL,
    -- Tag principal (ex. `waterway=waterfall`).
    osm_tag       text NOT NULL,
    location      geography(Point, 4326) NOT NULL,
    opening_hours text,
    website       text,
    phone         text,
    email         text,
    wikidata      text,
    wikipedia     text,
    PRIMARY KEY (osm_type, osm_id)
);
CREATE INDEX osm_places_location_idx ON osm_places USING gist(location);

-- Dernier import (une seule ligne) : réimport seulement si le fichier a changé.
CREATE TABLE osm_import (
    id            boolean PRIMARY KEY DEFAULT true CHECK (id),
    source        text NOT NULL,
    file_size     bigint,
    file_modified timestamptz,
    imported_at   timestamptz NOT NULL DEFAULT now(),
    place_count   integer NOT NULL
);

CREATE TABLE trip_dismissed_suggestions (
    trip_id      uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    osm_type     text NOT NULL CHECK (osm_type IN ('node', 'way', 'relation')),
    osm_id       bigint NOT NULL CHECK (osm_id > 0),
    -- Catégorie au moment du refus : les lieux semblables sont un peu moins proposés.
    category     text NOT NULL,
    dismissed_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (trip_id, osm_type, osm_id)
);
