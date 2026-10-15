-- Points d'intérêt (POI) d'un voyage.

CREATE TABLE pois (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trip_id       uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    name          text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    category      text NOT NULL CHECK (category IN
                      ('sight', 'nature', 'museum', 'food', 'activity', 'lodging', 'shopping', 'other')),
    location      geography(Point, 4326) NOT NULL,
    address       text NOT NULL DEFAULT '' CHECK (length(address) <= 500),
    -- Durée de visite estimée (0 pour un hébergement).
    duration_min  integer NOT NULL CHECK (duration_min BETWEEN 0 AND 720),
    -- Syntaxe OSM `opening_hours`, conservée telle quelle même si elle est invalide
    -- (l'interface le signale et propose de la corriger).
    opening_hours text CHECK (length(opening_hours) <= 500),
    -- Code pays ISO (jours fériés pour l'interprétation des horaires).
    country_code  text CHECK (country_code ~ '^[A-Z]{2}$'),
    website       text CHECK (length(website) <= 500),
    notes         text NOT NULL DEFAULT '' CHECK (length(notes) <= 2000),
    osm_type      text CHECK (osm_type IN ('node', 'way', 'relation')),
    osm_id        bigint CHECK (osm_id > 0),
    version       bigint NOT NULL DEFAULT 1,
    created_by    uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CHECK ((osm_type IS NULL) = (osm_id IS NULL))
);
CREATE TRIGGER pois_updated_at BEFORE UPDATE ON pois
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX pois_trip_id_idx ON pois(trip_id);
CREATE INDEX pois_location_idx ON pois USING gist(location);
-- Un même lieu OSM n'est ajouté qu'une fois par voyage.
CREATE UNIQUE INDEX pois_trip_osm_unique ON pois(trip_id, osm_type, osm_id) WHERE osm_id IS NOT NULL;
