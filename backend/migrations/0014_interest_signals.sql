-- Signaux d'intérêt des éléments Wikidata, à côté de leur notoriété : guide Wikivoyage,
-- patrimoine mondial de l'UNESCO, superficie (parcs et réserves) et consultations mensuelles
-- de leurs articles Wikipédia (fr, en, es). Affichés comme raisons des suggestions.

ALTER TABLE wikidata_notability
    ADD COLUMN wikivoyage     boolean NOT NULL DEFAULT false,
    ADD COLUMN world_heritage boolean NOT NULL DEFAULT false,
    ADD COLUMN area_km2       double precision CHECK (area_km2 > 0),
    ADD COLUMN monthly_views  integer NOT NULL DEFAULT 0 CHECK (monthly_views >= 0);

-- Les lignes existantes n'ont que les langues : à redemander au prochain rafraîchissement.
UPDATE wikidata_notability SET fetched_at = 'epoch';
