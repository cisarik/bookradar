CREATE TABLE sources (
    id INTEGER PRIMARY KEY CHECK (id > 0)
) STRICT;

CREATE TABLE editions (
    id INTEGER PRIMARY KEY CHECK (id > 0)
) STRICT;

CREATE TABLE offers (
    source_id INTEGER NOT NULL REFERENCES sources(id),
    id INTEGER NOT NULL CHECK (id > 0),
    PRIMARY KEY (source_id, id)
) STRICT;

CREATE TABLE observations (
    source_id INTEGER NOT NULL,
    offer_id INTEGER NOT NULL,
    id INTEGER NOT NULL CHECK (id > 0),

    subject TEXT NOT NULL
        CHECK (subject IN ('resolved','unresolved','conflicting')),
    edition_id INTEGER REFERENCES editions(id),
    language TEXT NOT NULL
        CHECK (language IN ('slovak','czech','mixed','unknown','conflicting')),

    observed_original TEXT,
    observed_offset INTEGER CHECK (observed_offset BETWEEN -64800 AND 64800),
    observed_s INTEGER,
    observed_ns INTEGER CHECK (observed_ns BETWEEN 0 AND 999999999),

    captured_original TEXT,
    captured_offset INTEGER CHECK (captured_offset BETWEEN -64800 AND 64800),
    captured_s INTEGER,
    captured_ns INTEGER CHECK (captured_ns BETWEEN 0 AND 999999999),

    ingested_s INTEGER NOT NULL,
    ingested_ns INTEGER NOT NULL CHECK (ingested_ns BETWEEN 0 AND 999999999),

    access TEXT NOT NULL CHECK (access IN (
        'readable','rate-limited','captcha','authentication-blocked',
        'timeout','transport-error','page-gone'
    )),
    listing TEXT NOT NULL CHECK (listing IN ('present','removed','uncertain')),
    availability TEXT NOT NULL
        CHECK (availability IN ('available','reserved','unavailable','unknown')),
    source_statement TEXT,

    evidence_method TEXT
        CHECK (evidence_method IN ('direct-browser','manual','agent-only')),
    evidence_reference TEXT,
    subject_covered INTEGER CHECK (subject_covered IN (0,1)),
    language_covered INTEGER CHECK (language_covered IN (0,1)),
    availability_covered INTEGER CHECK (availability_covered IN (0,1)),

    requested_url TEXT,
    resolved_url TEXT,
    redirect_reference TEXT,
    provenance_reference TEXT NOT NULL,
    uncertainty_rationale TEXT,

    PRIMARY KEY (source_id, offer_id, id),
    FOREIGN KEY (source_id, offer_id) REFERENCES offers(source_id, id),

    CHECK (
        (subject = 'resolved' AND edition_id IS NOT NULL)
        OR (subject IN ('unresolved','conflicting') AND edition_id IS NULL)
    ),
    CHECK ((observed_s IS NULL) = (observed_ns IS NULL)),
    CHECK (observed_offset IS NULL OR observed_s IS NOT NULL),
    CHECK ((captured_s IS NULL) = (captured_ns IS NULL)),
    CHECK (captured_offset IS NULL OR captured_s IS NOT NULL),
    CHECK (
        (evidence_method IS NULL AND evidence_reference IS NULL
            AND subject_covered IS NULL AND language_covered IS NULL
            AND availability_covered IS NULL)
        OR
        (evidence_method IS NOT NULL AND subject_covered IS NOT NULL
            AND language_covered IS NOT NULL
            AND availability_covered IS NOT NULL)
    )
) STRICT;

CREATE TABLE reviews (
    source_id INTEGER NOT NULL,
    offer_id INTEGER NOT NULL,
    observation_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    assessed_s INTEGER NOT NULL,
    assessed_ns INTEGER NOT NULL CHECK (assessed_ns BETWEEN 0 AND 999999999),
    recorded_s INTEGER NOT NULL,
    recorded_ns INTEGER NOT NULL CHECK (recorded_ns BETWEEN 0 AND 999999999),
    knowledge TEXT NOT NULL
        CHECK (knowledge IN ('unreviewed','supported','disputed','rejected')),
    reviewer_approval TEXT,
    assessor_reference TEXT NOT NULL,
    rationale TEXT NOT NULL,
    PRIMARY KEY (source_id, offer_id, observation_id, revision),
    FOREIGN KEY (source_id, offer_id, observation_id)
        REFERENCES observations(source_id, offer_id, id)
) STRICT;

CREATE INDEX observations_by_time
    ON observations(source_id, offer_id, observed_s, observed_ns, id);

CREATE TRIGGER observations_no_update BEFORE UPDATE ON observations
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER observations_no_delete BEFORE DELETE ON observations
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER reviews_no_update BEFORE UPDATE ON reviews
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER reviews_no_delete BEFORE DELETE ON reviews
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
