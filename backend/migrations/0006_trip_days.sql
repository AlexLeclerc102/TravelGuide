-- Réglages propres à un jour du voyage : moyens de transport et hébergement.
-- Une ligne n'existe que si le jour s'écarte des valeurs par défaut du voyage.

CREATE TABLE trip_days (
    trip_id     uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    date        date NOT NULL,
    -- NULL : le jour reprend `trips.default_modes`.
    modes       text[]
        CHECK (modes IS NULL OR (cardinality(modes) >= 1
               AND modes <@ ARRAY['walk', 'bike', 'car', 'transit'])),
    -- Hébergement du jour : point de départ et d'arrivée de la journée.
    base_poi_id uuid REFERENCES pois(id) ON DELETE SET NULL,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (trip_id, date)
);
CREATE TRIGGER trip_days_updated_at BEFORE UPDATE ON trip_days
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX trip_days_base_poi_id_idx ON trip_days(base_poi_id);
