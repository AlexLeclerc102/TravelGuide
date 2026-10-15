-- Itinéraire généré d'un voyage : un ordre de visite et des horaires par jour.
-- Les réglages et lieux utilisés au calcul sont conservés pour détecter un itinéraire
-- périmé (lieu modifié ou supprimé, modes ou hébergement changés, dates du voyage).

CREATE TABLE itineraries (
    trip_id      uuid PRIMARY KEY REFERENCES trips(id) ON DELETE CASCADE,
    -- Incrémentée à chaque génération (verrouillage optimiste des ajustements à venir).
    version      bigint NOT NULL DEFAULT 1,
    generated_at timestamptz NOT NULL DEFAULT now(),
    generated_by uuid REFERENCES users(id) ON DELETE SET NULL
);

CREATE TABLE itinerary_days (
    trip_id         uuid NOT NULL REFERENCES itineraries(trip_id) ON DELETE CASCADE,
    date            date NOT NULL,
    modes           text[] NOT NULL,
    -- Départ : hébergement de la veille (sinon celui du jour) ; arrivée : hébergement du jour.
    start_poi_id    uuid REFERENCES pois(id) ON DELETE SET NULL,
    end_poi_id      uuid REFERENCES pois(id) ON DELETE SET NULL,
    day_start       time NOT NULL,
    day_end         time NOT NULL,
    computed_at     timestamptz NOT NULL DEFAULT now(),
    -- Nombre d'étapes au calcul : une étape supprimée (lieu supprimé) se remarque.
    stop_count      smallint NOT NULL DEFAULT 0 CHECK (stop_count >= 0),
    -- Trajet de la dernière visite à l'hébergement du soir.
    back_mode       text CHECK (back_mode IN ('walk', 'bike', 'car', 'transit')),
    back_duration_s integer CHECK (back_duration_s >= 0),
    back_distance_m integer CHECK (back_distance_m >= 0),
    back_source     text CHECK (back_source IN ('router', 'straight_line', 'transit_estimate')),
    PRIMARY KEY (trip_id, date)
);
CREATE INDEX itinerary_days_start_poi_idx ON itinerary_days(start_poi_id);
CREATE INDEX itinerary_days_end_poi_idx ON itinerary_days(end_poi_id);

CREATE TABLE itinerary_stops (
    trip_id           uuid NOT NULL,
    date              date NOT NULL,
    position          smallint NOT NULL CHECK (position >= 0),
    poi_id            uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    -- Horaires en secondes depuis minuit (heure locale du voyage).
    arrival_s         integer NOT NULL CHECK (arrival_s BETWEEN 0 AND 86400),
    start_s           integer NOT NULL CHECK (start_s BETWEEN 0 AND 86400),
    end_s             integer NOT NULL CHECK (end_s BETWEEN 0 AND 86400),
    -- Trajet depuis l'étape précédente ; NULL pour une première visite sans départ.
    travel_mode       text CHECK (travel_mode IN ('walk', 'bike', 'car', 'transit')),
    travel_duration_s integer CHECK (travel_duration_s >= 0),
    travel_distance_m integer CHECK (travel_distance_m >= 0),
    travel_source     text CHECK (travel_source IN ('router', 'straight_line', 'transit_estimate')),
    -- Étape verrouillée à la main (ajustements manuels, incrément suivant).
    locked            boolean NOT NULL DEFAULT false,
    PRIMARY KEY (trip_id, date, position),
    UNIQUE (trip_id, poi_id),
    FOREIGN KEY (trip_id, date) REFERENCES itinerary_days(trip_id, date) ON DELETE CASCADE
);
CREATE INDEX itinerary_stops_poi_id_idx ON itinerary_stops(poi_id);

-- Lieux laissés de côté à la génération, avec la raison.
CREATE TABLE itinerary_unscheduled (
    trip_id uuid NOT NULL REFERENCES itineraries(trip_id) ON DELETE CASCADE,
    poi_id  uuid NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    reason  text NOT NULL CHECK (reason IN ('closed_all_days', 'opening_hours', 'no_time')),
    PRIMARY KEY (trip_id, poi_id)
);
CREATE INDEX itinerary_unscheduled_poi_id_idx ON itinerary_unscheduled(poi_id);
