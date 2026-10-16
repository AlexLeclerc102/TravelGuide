-- Envies du groupe (questionnaire de création du voyage) : centres d'intérêt, rythme et
-- budget d'hébergement. Elles orientent suggestions, régions et circuits types.

ALTER TABLE trips
    ADD COLUMN interests text[] NOT NULL DEFAULT '{}'
        CHECK (interests <@ ARRAY['nature', 'beach', 'sight', 'museum', 'activity', 'food', 'shopping']),
    ADD COLUMN pace text NOT NULL DEFAULT 'balanced'
        CHECK (pace IN ('relaxed', 'balanced', 'packed')),
    -- NULL : pas de préférence.
    ADD COLUMN budget text CHECK (budget IN ('budget', 'comfort'));
