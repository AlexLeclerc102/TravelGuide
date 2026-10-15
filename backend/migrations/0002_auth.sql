-- Comptes utilisateurs, sessions et liens magiques.

CREATE TABLE users (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email             citext NOT NULL UNIQUE CHECK (length(email) BETWEEN 3 AND 254),
    display_name      text NOT NULL CHECK (length(display_name) BETWEEN 1 AND 80),
    -- NULL pour les comptes sans mot de passe (invités, lien magique seul).
    password_hash     text,
    -- Abonnement simulé pour le MVP : seul `organizer` peut créer des voyages.
    plan              text NOT NULL DEFAULT 'free' CHECK (plan IN ('free', 'organizer')),
    email_verified_at timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Le jeton n'est jamais stocké en clair : uniquement son empreinte SHA-256.
CREATE TABLE sessions (
    token_hash   bytea PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent   text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);

CREATE TABLE magic_links (
    token_hash bytea PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at    timestamptz
);
CREATE INDEX magic_links_user_id_idx ON magic_links(user_id);
