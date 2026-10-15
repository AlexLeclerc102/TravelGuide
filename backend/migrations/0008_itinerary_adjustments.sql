-- Ajustements manuels de l'itinéraire : une étape placée à la main peut ne pas respecter
-- les horaires ou déborder de la journée (signalé), et un lieu peut être retiré à la main.

ALTER TABLE itinerary_stops
    ADD COLUMN issue text CHECK (issue IN ('outside_opening_hours', 'after_day_end')),
    -- Une journée surchargée à la main peut déborder après minuit.
    DROP CONSTRAINT itinerary_stops_arrival_s_check,
    DROP CONSTRAINT itinerary_stops_start_s_check,
    DROP CONSTRAINT itinerary_stops_end_s_check,
    ADD CONSTRAINT itinerary_stops_times_check
        CHECK (arrival_s >= 0 AND start_s >= arrival_s AND end_s >= start_s);

ALTER TABLE itinerary_unscheduled
    DROP CONSTRAINT itinerary_unscheduled_reason_check,
    ADD CONSTRAINT itinerary_unscheduled_reason_check
        CHECK (reason IN ('closed_all_days', 'opening_hours', 'no_time', 'removed'));
