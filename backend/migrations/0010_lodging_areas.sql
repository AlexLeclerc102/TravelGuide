-- Zone d'hébergement : un hébergement encore à choisir, représenté par une ville ou un
-- quartier (point approximatif). Il sert de départ et d'arrivée comme un hôtel, puis est
-- remplacé par l'hébergement retenu.

ALTER TABLE pois
    ADD COLUMN placeholder boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT pois_placeholder_is_lodging CHECK (NOT placeholder OR category = 'lodging');
