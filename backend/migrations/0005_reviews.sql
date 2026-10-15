-- Avis personnels des membres sur les POI : une note (1 à 5) et un commentaire par membre et par POI.

CREATE TABLE reviews (
    poi_id     uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    -- Supprimés avec le compte (décision RGPD de l'incrément 1).
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rating     smallint NOT NULL CHECK (rating BETWEEN 1 AND 5),
    comment    text NOT NULL DEFAULT '' CHECK (length(comment) <= 2000),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (poi_id, user_id)
);
CREATE TRIGGER reviews_updated_at BEFORE UPDATE ON reviews
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX reviews_user_id_idx ON reviews(user_id);
