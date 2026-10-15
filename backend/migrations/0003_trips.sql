-- Voyages, membres (rôles) et invitations par lien.

CREATE TABLE trips (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name          text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    description   text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    start_date    date NOT NULL,
    end_date      date NOT NULL,
    timezone      text NOT NULL DEFAULT 'America/Costa_Rica',
    -- Plage horaire quotidienne par défaut (budget de temps de chaque journée).
    day_start     time NOT NULL DEFAULT '09:00',
    day_end       time NOT NULL DEFAULT '19:00',
    default_modes text[] NOT NULL DEFAULT '{car}'
        CHECK (cardinality(default_modes) >= 1
               AND default_modes <@ ARRAY['walk', 'bike', 'car', 'transit']),
    -- Verrouillage optimiste : incrémenté à chaque modification.
    version       bigint NOT NULL DEFAULT 1,
    created_by    uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (end_date >= start_date AND end_date - start_date <= 60),
    CHECK (day_end > day_start)
);
CREATE TRIGGER trips_updated_at BEFORE UPDATE ON trips
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TABLE trip_members (
    trip_id   uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    user_id   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role      text NOT NULL CHECK (role IN ('organizer', 'editor', 'viewer')),
    joined_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (trip_id, user_id)
);
CREATE INDEX trip_members_user_id_idx ON trip_members(user_id);
-- Un seul organisateur (celui qui paie) par voyage.
CREATE UNIQUE INDEX trip_members_one_organizer ON trip_members(trip_id) WHERE role = 'organizer';

CREATE TABLE invitations (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    trip_id    uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE,
    role       text NOT NULL CHECK (role IN ('editor', 'viewer')),
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    max_uses   integer CHECK (max_uses > 0),
    uses       integer NOT NULL DEFAULT 0,
    revoked_at timestamptz
);
CREATE INDEX invitations_trip_id_idx ON invitations(trip_id);
