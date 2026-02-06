# md-db-rs Specification

Strict-format markdown files as a human-readable database. LLM agents are the primary programmatic consumer — token efficiency is critical.

## Core Concepts

### Documents
Markdown files with YAML frontmatter. Each document has a `type` field that maps to a schema-defined type. Documents link to each other via references in frontmatter.

### Schema (KDL)
A `.kdl` file defines the vocabulary: document types, fields, sections, relations, and reference formats. The schema is the single source of truth for validation. KDL v2 syntax (`#true`/`#false` for booleans).

### Relations
Defined once at schema level, available on ALL document types. Schema authors define their own relationship vocabulary — the tool doesn't hardcode any specific relationships.

```kdl
relation "supersedes" inverse="superseded_by" cardinality="one"
relation "enables" inverse="enabled_by" cardinality="many"
relation "depends_on" inverse="dependency_of" cardinality="many"
relation "related" cardinality="many"
```

- `inverse` — auto-generates the reverse field name (optional; omit for symmetric relations like "related")
- `cardinality` — `"one"` = single ref, `"many"` = ref array

### References
Two formats for cross-doc refs:
- **String ID**: `"ADR-005"`, `"OPP-012"` — resolved by matching against filenames uppercased
- **Relative path**: `"./adr-005.md"`, `"../gov/gov-003.md"` — resolved as filesystem paths

Ref format patterns defined in schema:
```kdl
ref-format {
    string-id pattern="^(ADR|OPP|GOV|INC)-\\d+$"
    relative-path pattern="\\.md$"
}
```

### Users and Teams
Users and teams are first-class entities defined in a separate config file (not in the schema). They can be referenced in frontmatter fields and table columns.

- Users: identified by `@handle` (e.g. `@onni`, `@alice`)
- Teams: identified by `@team/name` (e.g. `@team/platform`, `@team/security`)
- Users can belong to teams
- Teams can be part of other teams (nested/hierarchical membership)
- Both users and teams can have arbitrary extra attributes (email, role, slack channel, etc.)

User/team config file (YAML):
```yaml
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

teams:
  platform:
    name: Platform Team
    slack: "#platform"
    lead: onni

  security:
    name: Security Team
    slack: "#security"

  engineering:
    name: Engineering
    teams: [platform, security]  # teams can contain other teams
```

Schema can use `type="user"` or `type="user[]"` for fields and table columns:
```kdl
type "adr" {
    field "author" type="user" required=#true
    field "reviewers" type="user[]"
}
```

Table column types include `type="user"`:
```kdl
section "Action Items" {
    table {
        column "Owner" type="user" required=#true
        column "Action" type="string" required=#true
    }
}
```

Validation checks:
- User/team refs resolve to defined users/teams
- `type="user"` fields contain valid `@handle` values

## Document Type Definitions

Each type defines:

### Fields
Frontmatter field constraints:
```kdl
field "title" type="string" required=#true
field "status" type="enum" required=#true {
    values "proposed" "accepted" "rejected"
}
field "author" type="user" required=#true
field "tags" type="string[]"
field "count" type="number"
field "active" type="bool"
field "ref_field" type="ref"
field "ref_array" type="ref[]"
```

Supported types: `string`, `number`, `bool`, `enum`, `ref`, `ref[]`, `string[]`, `user`, `user[]`

Fields can have:
- `required=#true` — must be present
- `pattern="regex"` — value must match regex

### Sections
Required document structure (heading hierarchy):
```kdl
section "Decision" required=#true
section "Consequences" required=#true {
    section "Positive" required=#true
    section "Negative"
}
```

Sections can contain table constraints:
```kdl
section "Alternatives" {
    table required=#true {
        column "Option" type="string" required=#true
        column "Score" type="number"
    }
}
```

## CLI Commands

### `get` — token-efficient reads
```sh
md-db get <FILE> --field status                           # bare value
md-db get <FILE> --frontmatter                            # full YAML
md-db get <FILE> --frontmatter --format json              # JSON
md-db get <FILE> --section Decision                       # section markdown
md-db get <FILE> --section Decision --format text         # plain text
md-db get <FILE> --section Options --table 0              # table as text
md-db get <FILE> --section Options --table 0 --format json # table as JSON
md-db get <FILE> --section Options --table 0 --cell Score,2 # single cell
md-db get <FILE> --format json                            # entire doc as JSON
```

### `list` — query the database
```sh
md-db list docs/                                  # all .md files
md-db list docs/ --pattern "adr-*.md"             # glob filter
md-db list docs/ --field status=accepted           # frontmatter filter
md-db list docs/ --has-field superseded_by         # field existence
md-db list docs/ --field status=accepted --field author=@onni  # AND
md-db list docs/ --format json                     # full frontmatter
md-db list docs/ --format json --fields title,status # selected fields
```

### `validate` — schema enforcement
```sh
md-db validate docs/ --schema schema.kdl          # text report
md-db validate docs/ --schema schema.kdl --format json  # JSON report
```

Validation checks:
- Required fields present
- Field types match (string, number, bool, enum, ref, user)
- Enum values in allowed set
- Pattern matches (regex)
- Required sections present
- Required tables present with required columns
- Relation fields contain valid refs
- Cross-doc references resolve (string IDs and relative paths)
- User/team refs resolve to defined users/teams

Error output format:
```
tests/fixtures/adr-002.md:
  error[F010]: missing required field "date"
    --> frontmatter
    = hint: add 'date: <string>' to frontmatter

  error[S010]: missing required section "Consequences > Positive"
    --> document body
    = hint: add heading: "# Positive" or "## Positive"
```

Error codes:
- `F0xx` — frontmatter errors (missing field, type mismatch, enum, pattern)
- `S0xx` — section/structure errors (missing section, missing table, missing column)
- `R0xx` — reference errors (broken ref, unresolved ID, bad format)

## Architecture

AST-first, no regex for content. All markdown manipulation via comrak AST nodes.

### Partial read flow
1. `gray_matter` splits frontmatter from body
2. `comrak::parse_document` with `sourcepos=true` builds AST
3. Walk AST to find target heading node by exact text match
4. Use `sourcepos` byte offsets to slice original body text — zero re-serialization
5. Return only requested content

### Project structure
```
crates/
  md-db/           # library
    src/
      lib.rs
      error.rs            # thiserror types
      document.rs         # Document: frontmatter + body, load/save
      frontmatter.rs      # parse/get/set YAML fields by dotted path
      ast_util.rs         # comrak helpers
      section.rs          # Section extraction via sourcepos
      table.rs            # Table read from AST nodes
      discovery.rs        # find files by glob + frontmatter filters
      output.rs           # text|markdown|json formatters
      schema.rs           # KDL schema parser
      validation.rs       # validation engine
  md-db-cli/       # binary
    src/
      main.rs
      commands/
        get.rs
        list.rs
        validate.rs
```

## Later Phases (not yet implemented)
- `set` — targeted mutations (field, section, table cell)
- `new` — create from template
- `links` — reference integrity checking / graph traversal
- `query` — filter DSL across docs
- `init` — create workspace config
- Workspace config (TOML) for default schema path, doc directories, user config path
- Inverse relation consistency checking (if A supersedes B, B should have superseded_by A)
