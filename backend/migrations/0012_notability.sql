-- Notoriété des éléments Wikidata (nombre de langues de leur article Wikipédia), rafraîchie
-- en tâche de fond : classe les lieux de tout le pays sans interroger Wikidata à chaque calcul.
-- Indexée par élément : survit aux réimports de `osm_places`.

CREATE TABLE wikidata_notability (
    qid        text PRIMARY KEY CHECK (qid ~ '^Q[0-9]+$'),
    languages  integer NOT NULL CHECK (languages >= 0),
    fetched_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX osm_places_wikidata_idx ON osm_places (wikidata) WHERE wikidata IS NOT NULL;
