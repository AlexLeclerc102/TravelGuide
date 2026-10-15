-- Coordonnées et références d'un lieu, reprises des tags OSM (`phone`, `email`, `wikidata`,
-- `wikipedia`) : liens d'appel, d'email et vers Wikipédia sur la fiche du lieu.

ALTER TABLE pois
    ADD COLUMN phone     text CHECK (length(phone) <= 50),
    ADD COLUMN email     text CHECK (length(email) <= 254),
    ADD COLUMN wikidata  text CHECK (wikidata ~ '^Q[1-9][0-9]{0,11}$'),
    -- Format OSM `langue:Titre` (ex. `es:Volcán Arenal`).
    ADD COLUMN wikipedia text CHECK (length(wikipedia) <= 300);
