-- Schema definition for Architecture Decision Records
-- Custom SQL-ish DSL: familiar, expressive, single-purpose

CREATE TYPE adr (
    -- Frontmatter fields
    title       STRING      NOT NULL,
    status      ENUM(
                    'proposed',
                    'accepted',
                    'rejected',
                    'deprecated',
                    'superseded'
                )           NOT NULL,
    author      STRING      NOT NULL    CHECK (author LIKE '@%'),
    reviewers   STRING[],
    tags        STRING[],
    superseded_by REF,
    related     REF[],

    -- Required sections (heading hierarchy)
    SECTION 'Decision'                  NOT NULL,
    SECTION 'Consequences'              NOT NULL (
        SECTION 'Positive'              NOT NULL,
        SECTION 'Negative'
    ),
    SECTION 'Alternatives Considered' (
        TABLE NOT NULL (
            'Option'    STRING  NOT NULL,
            'Score'     NUMBER,
            'Notes'     STRING
        )
    )
);

-- Reference resolution rules
CREATE REF FORMAT (
    STRING_ID   PATTERN '^ADR-\d+$',
    REL_PATH    PATTERN '\.md$'
);
