CREATE TABLE research_inputs (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    raw_bytes BLOB NOT NULL,
    canonical_bytes BLOB,
    ingested_s INTEGER NOT NULL,
    ingested_ns INTEGER NOT NULL CHECK (ingested_ns BETWEEN 0 AND 999999999),
    disposition TEXT NOT NULL CHECK (disposition IN ('complete','partial','quarantined')),
    reported_outcome TEXT CHECK (
        reported_outcome IS NULL
        OR reported_outcome IN ('succeeded','partial','failed','unknown')
    ),
    rule_version TEXT NOT NULL,
    issues TEXT NOT NULL
) STRICT;

CREATE TABLE research_runs (
    producer_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    canonical_context BLOB NOT NULL,
    first_receipt_id INTEGER NOT NULL REFERENCES research_inputs(id),
    PRIMARY KEY (producer_id, run_id)
) STRICT;

CREATE TABLE research_findings (
    receipt_id INTEGER NOT NULL REFERENCES research_inputs(id),
    position INTEGER NOT NULL CHECK (position >= 0),
    producer_id TEXT,
    run_id TEXT,
    finding_id TEXT,
    canonical_finding BLOB,
    outcome TEXT NOT NULL CHECK (outcome IN ('admitted','replayed','rejected','quarantined')),
    reason TEXT NOT NULL,
    knowledge TEXT CHECK (knowledge IS NULL OR knowledge IN (
        'unreviewed','supported','disputed','rejected'
    )),
    original_receipt_id INTEGER,
    original_position INTEGER,
    source_ref TEXT,
    listing_ref TEXT,
    listing_generation TEXT,
    source_id INTEGER,
    offer_id INTEGER,
    observation_id INTEGER,
    PRIMARY KEY (receipt_id, position),
    FOREIGN KEY (source_id, offer_id, observation_id)
        REFERENCES observations(source_id, offer_id, id)
) STRICT;

CREATE INDEX research_findings_by_identity
    ON research_findings(producer_id, run_id, finding_id);

CREATE TRIGGER inputs_no_update BEFORE UPDATE ON research_inputs
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER inputs_no_delete BEFORE DELETE ON research_inputs
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER runs_no_update BEFORE UPDATE ON research_runs
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER runs_no_delete BEFORE DELETE ON research_runs
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER findings_no_update BEFORE UPDATE ON research_findings
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TRIGGER findings_no_delete BEFORE DELETE ON research_findings
BEGIN SELECT RAISE(ABORT, 'append-only'); END;
