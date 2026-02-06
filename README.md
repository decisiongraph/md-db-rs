# md-db

Strict-format markdown files as a human-readable database. LLM agents are the primary consumer — token efficiency is critical.

Markdown documents with YAML frontmatter form a queryable, validatable document store. Documents link to each other via typed relations. A KDL schema enforces structure, field types, and cross-document reference integrity.

## Install

```sh
cargo install --path crates/md-db-cli
```

## Quick Start

Given a directory of markdown documents:

```
docs/
  adr-001.md    # Architecture Decision Record
  adr-002.md
  opp-001.md    # Product Opportunity
  gov-001.md    # Governance Policy
  inc-001.md    # Incident Report
  schema.kdl    # Schema definition
  users.yaml    # User/team config
```

### Read a field

```sh
$ md-db get docs/adr-001.md --field status
accepted

$ md-db get docs/inc-001.md --field severity
sev2

$ md-db get docs/opp-001.md --field priority
high
```

### Read frontmatter

```sh
$ md-db get docs/adr-001.md --frontmatter
author: '@onni'
date: '2025-01-10'
enables:
- OPP-001
related:
- ADR-002
reviewers:
- '@alice'
- '@bob'
status: accepted
tags:
- database
- infrastructure
title: Use PostgreSQL
triggers:
- GOV-001
type: adr

$ md-db get docs/adr-001.md --frontmatter --format json
{"author":"@onni","date":"2025-01-10","enables":["OPP-001"],...}
```

### Read a section

```sh
$ md-db get docs/adr-001.md --section Decision
## Rationale

PostgreSQL offers the best combination of reliability, performance, and features.

## Alternatives Considered

| Option | Score | Notes |
|--------|-------|-------|
| PostgreSQL | 9 | Best overall |
| MySQL | 7 | Good but fewer features |
| SQLite | 5 | Not suitable for production |

$ md-db get docs/adr-001.md --section Decision --format text
Rationale

PostgreSQL offers the best combination of reliability, performance, and features.

...
```

### Read a table

```sh
$ md-db get docs/adr-001.md --section "Alternatives Considered" --table 0
Option     | Score | Notes
-----------+-------+------------------------------
PostgreSQL | 9     | Best overall
MySQL      | 7     | Good but fewer features
SQLite     | 5     | Not suitable for production

$ md-db get docs/adr-001.md --section "Alternatives Considered" --table 0 --format json
[
  {"Option": "PostgreSQL", "Score": "9", "Notes": "Best overall"},
  {"Option": "MySQL", "Score": "7", "Notes": "Good but fewer features"},
  {"Option": "SQLite", "Score": "5", "Notes": "Not suitable for production"}
]
```

### Read a single cell

```sh
$ md-db get docs/adr-001.md --section "Alternatives Considered" --table 0 --cell Score,0
9
```

### Entire document as JSON

```sh
$ md-db get docs/adr-001.md --format json
{
  "frontmatter": {"title": "Use PostgreSQL", "status": "accepted", ...},
  "sections": [...],
  "body": "..."
}
```

## List & Filter

```sh
# All markdown files
$ md-db list docs/
docs/adr-001.md
docs/adr-002.md
docs/opp-001.md
docs/gov-001.md
docs/inc-001.md

# Glob pattern
$ md-db list docs/ --pattern "adr-*.md"
docs/adr-001.md
docs/adr-002.md
docs/adr-003.md

# Filter by frontmatter field
$ md-db list docs/ --field status=accepted
docs/adr-001.md

# Filter by field existence
$ md-db list docs/ --has-field superseded_by
docs/adr-003.md

# Combine filters (AND)
$ md-db list docs/ --field type=adr --has-field related
docs/adr-001.md
docs/adr-002.md

# JSON output with selected fields
$ md-db list docs/ --field type=adr --format json --fields title,status
[
  {"path": "docs/adr-001.md", "title": "Use PostgreSQL", "status": "accepted"},
  {"path": "docs/adr-002.md", "title": "Use REST API", "status": "proposed"},
  {"path": "docs/adr-003.md", "title": "Use Redis for Caching", "status": "superseded"}
]
```

## Schema Validation

### Define a schema (KDL)

```kdl
// schema.kdl

// Relations: defined once, available on all document types
relation "supersedes" inverse="superseded_by" cardinality="one"
relation "enables" inverse="enabled_by" cardinality="many"
relation "triggers" inverse="caused_by" cardinality="many"
relation "blocks" inverse="blocked_by" cardinality="many"
relation "related" cardinality="many"

// Document types
type "adr" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true {
        values "proposed" "accepted" "rejected" "deprecated" "superseded"
    }
    field "author" type="user" required=#true
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$"
    field "reviewers" type="user[]"
    field "tags" type="string[]"

    section "Decision" required=#true
    section "Consequences" required=#true {
        section "Positive" required=#true
        section "Negative"
    }
    section "Alternatives Considered" {
        table {
            column "Option" type="string" required=#true
            column "Score" type="number"
            column "Notes" type="string"
        }
    }
}

type "inc" {
    field "title" type="string" required=#true
    field "severity" type="enum" required=#true {
        values "sev1" "sev2" "sev3" "sev4"
    }
    field "commander" type="user" required=#true
    field "responders" type="user[]"
    field "affected_systems" type="string[]" required=#true

    section "Summary" required=#true
    section "Timeline" required=#true {
        table {
            column "Time" type="string" required=#true
            column "Event" type="string" required=#true
        }
    }
    section "Action Items" required=#true {
        table {
            column "Action" type="string" required=#true
            column "Owner" type="user" required=#true
            column "Status" type="string"
        }
    }
}

ref-format {
    string-id pattern="^(ADR|OPP|GOV|INC)-\\d+$"
    relative-path pattern="\\.md$"
}
```

### Field types

| Type | YAML example | Description |
|------|-------------|-------------|
| `string` | `title: "Use PostgreSQL"` | Any string |
| `number` | `duration_minutes: 93` | Integer or float |
| `bool` | `active: true` | Boolean |
| `enum` | `status: accepted` | One of a defined set |
| `ref` | `superseded_by: "ADR-005"` | Cross-doc reference |
| `ref[]` | `enables: ["OPP-001"]` | Array of refs |
| `string[]` | `tags: [database, infra]` | String array |
| `user` | `author: "@onni"` | User/team reference |
| `user[]` | `reviewers: ["@alice", "@bob"]` | User/team ref array |

Fields support:
- `required=#true` — must be present
- `pattern="regex"` — value must match

### Run validation

```sh
$ md-db validate docs/ --schema schema.kdl
result: 0 error(s), 0 warning(s)

$ md-db validate docs/ --schema schema.kdl --users users.yaml
docs/adr-003.md:
  warning[R011]: unresolved reference "ADR-005" in "superseded_by"
    --> frontmatter.superseded_by
    = hint: no document with matching ID found in scope

result: 0 error(s), 1 warning(s)

$ md-db validate docs/ --schema schema.kdl --format json
{"errors": 0, "warnings": 1, "ok": true, "files": [...]}
```

### Error codes

| Code | Category | Example |
|------|----------|---------|
| `F010` | Missing required field | `missing required field "date"` |
| `F020` | Type mismatch | `field "count" expected number, got string` |
| `F021` | Invalid enum | `field "status" has invalid value "banana"` |
| `F030` | Pattern mismatch | `field "date" value "nope" doesn't match pattern` |
| `S010` | Missing section | `missing required section "Decision"` |
| `S020` | Missing table | `section "Timeline" requires a table` |
| `S021` | Missing column | `table missing required column "Owner"` |
| `R001` | Bad ref format | `ref doesn't match any ref-format` |
| `R010` | Broken file ref | `broken file reference "./missing.md"` |
| `R011` | Unresolved ID | `unresolved reference "ADR-999"` |
| `U010` | Invalid user format | `not a valid user reference` |
| `U011` | Unknown user/team | `references unknown user/team "@ghost"` |

## Relations

Relations define typed, directional links between documents. Defined once at schema level, available on all document types.

```kdl
relation "supersedes" inverse="superseded_by" cardinality="one"
relation "enables" inverse="enabled_by" cardinality="many"
relation "triggers" inverse="caused_by" cardinality="many"
relation "blocks" inverse="blocked_by" cardinality="many"
relation "related" cardinality="many"
```

- `inverse` — auto-generates the reverse field name. Optional; omit for symmetric relations.
- `cardinality` — `"one"` produces a single ref field, `"many"` produces a ref array.

### Example: linked documents

```
ADR-001 ──enables──> OPP-001 ──blocked_by──> GOV-001
   │                    │                       │
   ├──triggers──> GOV-001                       │
   ├──related──> ADR-002 <──enables── GOV-001   │
   │                                            │
   └────────────<──caused_by── INC-001 ──related──┘
```

In `adr-001.md`:
```yaml
---
type: adr
title: Use PostgreSQL
status: accepted
author: "@onni"
enables:
  - "OPP-001"
triggers:
  - "GOV-001"
related:
  - "ADR-002"
---
```

In `opp-001.md` (the other side):
```yaml
---
type: opp
title: Real-time Collaboration
enabled_by:
  - "ADR-001"
  - "ADR-002"
blocked_by:
  - "GOV-001"
---
```

### Reference formats

Two formats supported:
- **String ID**: `"ADR-005"` — matched against filenames uppercased (`adr-005.md` -> `ADR-005`)
- **Relative path**: `"./adr-005.md"` — resolved as filesystem path

Define patterns in schema:
```kdl
ref-format {
    string-id pattern="^(ADR|OPP|GOV|INC)-\\d+$"
    relative-path pattern="\\.md$"
}
```

## Users & Teams

Users and teams are defined in a YAML config file, separate from the schema.

```yaml
# users.yaml
users:
  onni:
    name: Onni Hakala
    email: onni@flaky.build
    teams: [platform, leadership]
    role: staff-engineer

  alice:
    name: Alice Smith
    email: alice@example.com
    teams: [platform]

  bob:
    name: Bob Jones
    teams: [security]

teams:
  platform:
    name: Platform Team
    slack: "#platform"
    lead: onni

  security:
    name: Security Team

  engineering:
    name: Engineering
    teams: [platform, security]  # nested: teams can contain teams
```

- Users: `@handle` (e.g. `@onni`, `@alice`)
- Teams: `@team/name` (e.g. `@team/platform`)
- Teams can contain other teams (hierarchical membership)
- Both support arbitrary extra attributes

Use in schema:
```kdl
field "author" type="user" required=#true
field "reviewers" type="user[]"

section "Action Items" {
    table {
        column "Owner" type="user" required=#true
    }
}
```

Use in documents:
```yaml
---
author: "@onni"
reviewers:
  - "@alice"
  - "@team/platform"
---
```

Validate with `--users`:
```sh
$ md-db validate docs/ --schema schema.kdl --users users.yaml
```

## Document Examples

### ADR (Architecture Decision Record)

```markdown
---
type: adr
title: Use PostgreSQL
status: accepted
author: "@onni"
date: "2025-01-10"
reviewers:
  - "@alice"
  - "@bob"
tags:
  - database
  - infrastructure
enables:
  - "OPP-001"
triggers:
  - "GOV-001"
related:
  - "ADR-002"
---

# Decision

We will use PostgreSQL as our primary database.

## Alternatives Considered

| Option | Score | Notes |
|--------|-------|-------|
| PostgreSQL | 9 | Best overall |
| MySQL | 7 | Good but fewer features |
| SQLite | 5 | Not suitable for production |

# Consequences

## Positive

Reliable ACID transactions and great ecosystem.

## Negative

More operational complexity than SQLite.
```

### INC (Incident Report)

```markdown
---
type: inc
title: Database Connection Pool Exhaustion
status: postmortem
severity: sev2
commander: "@onni"
responders:
  - "@alice"
  - "@bob"
started_at: "2025-01-20T14:32:00Z"
resolved_at: "2025-01-20T16:05:00Z"
duration_minutes: 93
affected_systems:
  - api-gateway
  - user-service
customer_impact: degraded
caused_by: "ADR-001"
triggers:
  - "ADR-003"
enables:
  - "OPP-001"
---

# Summary

Connection pool exhausted due to leaked connections from ORM migration.

# Impact

- 40% of API requests experienced >10s latency
- 5% of requests failed with 503 errors

# Timeline

| Time | Event | Actor |
|------|-------|-------|
| 14:32 | Alerts fire: API p99 >5s | PagerDuty |
| 14:35 | Incident declared | @onni |
| 14:50 | Root cause found | @alice |
| 15:25 | Hotfix deployed | @bob |
| 16:05 | Resolved | @onni |

# Root Cause

Missing connection.close() in error paths of new ORM code.

# Resolution

Added explicit connection.close() in finally blocks.

# Action Items

| Action | Owner | Due | Status |
|--------|-------|-----|--------|
| Add connection leak detection to CI | @alice | 2025-02-01 | done |
| Load test ORM migration paths | @bob | 2025-02-05 | in-progress |

# Lessons Learned

- ORMs that don't auto-close connections in error paths are dangerous
```

## Write Commands

### Set a field

```sh
$ md-db set docs/adr-001.md --field status=deprecated
```

### Replace section content

```sh
$ md-db set docs/adr-001.md --section "Decision" --content "We chose MongoDB instead."
```

### Append to section

```sh
$ md-db set docs/adr-001.md --section "Consequences" --append "Additional risk: vendor lock-in."
```

### Update a table cell

```sh
$ md-db set docs/adr-001.md --section "Alternatives Considered" --table 0 --cell Score,0 --value 10
```

### Add a table row

```sh
$ md-db set docs/adr-001.md --section "Alternatives Considered" --table 0 --add-row "CockroachDB,8,Distributed SQL"
```

### Dry run (print to stdout)

```sh
$ md-db set docs/adr-001.md --field status=deprecated --dry-run
```

## Deprecate

Set a document's status to deprecated, optionally marking it as superseded:

```sh
$ md-db deprecate docs/adr-001.md --schema schema.kdl --superseded-by ADR-005

$ md-db deprecate docs/adr-001.md --schema schema.kdl --superseded-by ADR-005 --dir docs/ --dry-run
```

## Create New Documents

Generate documents from schema type definitions:

```sh
# Print to stdout
$ md-db new --type adr --schema schema.kdl

# With pre-filled fields
$ md-db new --type adr --schema schema.kdl --field title="Use MongoDB" --field status=proposed

# Auto-generate next ID and write to file
$ md-db new --type adr --schema schema.kdl --dir docs/ --auto-id --fill
```

## Inspect

Frontmatter + sections + validation in a single call:

```sh
$ md-db inspect docs/adr-001.md --schema schema.kdl
$ md-db inspect docs/adr-001.md --schema schema.kdl --users users.yaml --format json
$ echo '---\ntype: adr\n...' | md-db inspect --stdin --schema schema.kdl
```

## Describe Schema

Explore schema types, fields, sections, and relations:

```sh
# List all types
$ md-db describe --schema schema.kdl

# Show a specific type
$ md-db describe --schema schema.kdl --type adr

# Show a specific field
$ md-db describe --schema schema.kdl --type adr --field status

# Show all relations
$ md-db describe --schema schema.kdl --relations

# Export full schema as JSON
$ md-db describe --schema schema.kdl --export --format json
```

## Refs & Backlinks

Query forward references or backlinks for a document:

```sh
# Outgoing refs from a document
$ md-db refs docs/ --schema schema.kdl --from ADR-001

# Backlinks to a document
$ md-db refs docs/ --schema schema.kdl --to GOV-001

# Transitive refs (depth 2)
$ md-db refs docs/ --schema schema.kdl --from ADR-001 --depth 2

$ md-db refs docs/ --schema schema.kdl --to GOV-001 --format json
```

## Graph Export

Export the document link graph:

```sh
# Mermaid (default)
$ md-db graph docs/ --schema schema.kdl
graph LR
  ADR-001 -->|enables| OPP-001
  ADR-001 -->|triggers| GOV-001
  ...

# DOT (Graphviz)
$ md-db graph docs/ --schema schema.kdl --format dot

# JSON
$ md-db graph docs/ --schema schema.kdl --format json

# Filter by type
$ md-db graph docs/ --schema schema.kdl --type adr
```

## Architecture

**AST-first, no regex for content.** All markdown manipulation via [comrak](https://github.com/kivikakk/comrak) AST nodes with `sourcepos` byte offsets for zero-copy section extraction.

### Read flow

1. `gray_matter` splits frontmatter from body
2. `comrak::parse_document` builds AST with source positions
3. Walk AST to find target heading by exact text match
4. Use `sourcepos` byte offsets to slice original body — zero re-serialization
5. Return only requested content

### Project structure

```
crates/
  md-db/           # library
    src/
      lib.rs
      error.rs            # thiserror error types
      document.rs         # Document: load, parse, section access
      frontmatter.rs      # YAML frontmatter parsing
      ast_util.rs         # comrak AST helpers
      section.rs          # Section extraction via sourcepos
      table.rs            # Table parsing from AST
      discovery.rs        # File discovery with glob + filters
      output.rs           # text|markdown|json formatters
      schema.rs           # KDL schema parser
      graph.rs            # Document link graph (mermaid, DOT, JSON)
      template.rs         # New document generation from schema
      users.rs            # User/team config loader
      validation.rs       # Validation engine
  md-db-cli/       # binary
    src/
      main.rs
      commands/
        batch.rs
        deprecate.rs
        describe.rs
        diff.rs
        export.rs
        fix.rs
        get.rs
        graph.rs
        hook.rs
        init.rs
        inspect.rs
        list.rs
        mcp.rs
        migrate.rs
        new.rs
        refs.rs
        rename.rs
        search.rs
        set.rs
        stats.rs
        sync.rs
        validate.rs
        watch.rs
```

### CLI commands

| Command | Description |
|---------|-------------|
| `get` | Read fields, sections, table cells from a document |
| `set` | Update fields, sections, table cells in a document |
| `list` | List/filter markdown files by frontmatter |
| `validate` | Validate documents against a KDL schema |
| `inspect` | Frontmatter + sections + validation in one call |
| `new` | Create a new document from a schema type |
| `deprecate` | Set status to deprecated, optionally mark superseded |
| `describe` | Explore schema types, fields, sections, relations |
| `refs` | Show forward refs or backlinks for a document |
| `graph` | Export document link graph (mermaid, DOT, JSON) |
| `batch` | Apply field mutations to all docs matching a filter |
| `diff` | Show structural diff between two document versions |
| `export` | Export documents to a static HTML site |
| `fix` | Auto-fix common validation errors |
| `hook` | Install or uninstall a git pre-commit hook |
| `init` | Scaffold a new md-db project with schema and dirs |
| `mcp` | Start MCP (Model Context Protocol) server over stdio |
| `migrate` | Detect schema changes and migrate documents |
| `rename` | Rename a document ID and cascade-update all refs |
| `search` | Full-text search across content and frontmatter |
| `stats` | Show document set health overview |
| `sync` | Sync bidirectional relations (add missing inverses) |
| `watch` | Watch directory and re-validate on file changes |
| `completions` | Generate shell completions (bash, zsh, fish, etc.) |

### Dependencies

| Crate | Purpose |
|-------|---------|
| comrak | Markdown AST (arena-based, GFM tables, sourcepos) |
| gray_matter | Frontmatter extraction |
| kdl | KDL v2 schema parsing |
| clap | CLI (derive) |
| serde_yaml / serde_json | Data serialization |
| regex | Pattern validation |
| walkdir + glob | File discovery |
| thiserror | Error types |

## Library Usage

```rust
use md_db::document::Document;
use md_db::schema::Schema;
use md_db::users::UserConfig;
use md_db::validation;

// Load and query a document
let doc = Document::from_file("docs/adr-001.md")?;
let fm = doc.frontmatter()?;
let status = fm.get_display("status").unwrap();     // "accepted"
let author = fm.get_display("author").unwrap();      // "@onni"

// Read a section
let decision = doc.get_section("Decision")?;
let text = decision.text();  // plain text, no markdown

// Read nested section
let positive = doc.get_section_by_path(&["Consequences", "Positive"])?;

// Read a table
let section = doc.get_section("Alternatives Considered")?;
let table = &section.tables()[0];
let score = table.get_cell("Score", 0);              // Some("9")
let json = table.to_json();                           // [{...}, ...]

// Full document as JSON
let json = doc.to_json();

// Discover files with filters
use md_db::discovery::{self, Filter};
let adrs = discovery::discover_files(
    "docs/",
    Some("adr-*.md"),
    &[Filter::FieldEquals {
        key: "status".into(),
        value: "accepted".into(),
    }],
)?;

// Validate
let schema = Schema::from_file("schema.kdl")?;
let users = UserConfig::from_file("users.yaml")?;
let result = validation::validate_directory("docs/", &schema, None, Some(&users))?;
if !result.is_ok() {
    eprintln!("{}", result.to_report());
}
```
