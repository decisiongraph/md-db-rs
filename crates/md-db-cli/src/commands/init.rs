use std::fs;
use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Output directory
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// Preset: minimal, adr, full
    #[arg(long, default_value = "minimal")]
    pub preset: String,
}

pub fn run(args: &InitArgs) -> Result<(), Box<dyn std::error::Error>> {
    let dir = &args.dir;
    fs::create_dir_all(dir)?;

    let schema_path = dir.join("schema.kdl");
    if schema_path.exists() {
        return Err("schema.kdl already exists — aborting".into());
    }

    let schema = match args.preset.as_str() {
        "adr" => adr_preset(),
        "full" => full_preset(),
        "minimal" => minimal_preset(),
        other => return Err(format!("unknown preset '{other}', expected: minimal, adr, full").into()),
    };

    fs::write(&schema_path, schema)?;

    let users_path = dir.join("users.yaml");
    fs::write(&users_path, users_template())?;

    // Create directories based on preset
    match args.preset.as_str() {
        "adr" => {
            fs::create_dir_all(dir.join("docs/architecture"))?;
        }
        "full" => {
            fs::create_dir_all(dir.join("docs/architecture"))?;
            fs::create_dir_all(dir.join("docs/incidents"))?;
            fs::create_dir_all(dir.join("docs/governance"))?;
            fs::create_dir_all(dir.join("docs/opportunities"))?;
        }
        _ => {
            fs::create_dir_all(dir.join("docs"))?;
        }
    }

    println!("Initialized md-db project in {}", dir.display());
    println!("  schema: {}", schema_path.display());
    println!("  users:  {}", users_path.display());
    println!("\nPreset: {}", args.preset);
    println!("Edit schema.kdl to define your document types.");

    Ok(())
}

fn minimal_preset() -> String {
    r#"// md-db schema — edit to define your document types
// See: https://github.com/decisiongraph/md-db-rs

ref-format {
    string-id pattern="^[A-Z]+-\\d+$"
    relative-path pattern="\\.md$"
}

type "doc" description="Generic document" folder="docs" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true default="draft" {
        values "draft" "published" "archived"
    }
    field "author" type="user" required=#true
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"
    field "tags" type="string[]"
}
"#
    .to_string()
}

fn adr_preset() -> String {
    r#"// md-db schema — Architecture Decision Records
// See: https://github.com/decisiongraph/md-db-rs

ref-format {
    string-id pattern="^ADR-\\d+$"
    relative-path pattern="\\.md$"
}

relation "supersedes" inverse="superseded_by" cardinality="one" description="Replaces a previous decision"
relation "related" cardinality="many" description="Related decisions"

type "adr" description="Architecture Decision Record" folder="docs/architecture" {
    field "title" type="string" required=#true description="Short decision summary"
    field "status" type="enum" required=#true default="proposed" description="Lifecycle state" {
        values "proposed" "accepted" "rejected" "deprecated" "superseded"
    }
    field "author" type="user" required=#true description="Decision maker"
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"
    field "tags" type="string[]"

    section "Context" required=#true description="Problem statement and constraints" {
        content min-paragraphs=1
    }
    section "Decision" required=#true description="The decision and rationale" {
        content min-paragraphs=1
    }
    section "Consequences" required=#true {
        section "Positive" required=#true
        section "Negative"
    }
}
"#
    .to_string()
}

fn full_preset() -> String {
    r#"// md-db schema — Full document management
// See: https://github.com/decisiongraph/md-db-rs

ref-format {
    string-id pattern="^(ADR|INC|GOV|OPP)-\\d+$"
    relative-path pattern="\\.md$"
}

relation "supersedes" inverse="superseded_by" cardinality="one"
relation "enables" inverse="enabled_by" cardinality="many"
relation "depends_on" inverse="dependency_of" cardinality="many"
relation "related" cardinality="many"
relation "triggers" inverse="triggered_by" cardinality="many"

type "adr" description="Architecture Decision Record" folder="docs/architecture" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true default="proposed" {
        values "proposed" "accepted" "rejected" "deprecated" "superseded"
    }
    field "author" type="user" required=#true
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"
    field "tags" type="string[]"

    section "Context" required=#true {
        content min-paragraphs=1
    }
    section "Decision" required=#true {
        content min-paragraphs=1
    }
    section "Consequences" required=#true {
        section "Positive" required=#true
        section "Negative"
    }
}

type "inc" description="Incident Report" folder="docs/incidents" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true default="open" {
        values "open" "investigating" "mitigated" "resolved"
    }
    field "severity" type="enum" required=#true {
        values "sev1" "sev2" "sev3" "sev4"
    }
    field "commander" type="user"
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"

    section "Summary" required=#true
    section "Timeline" required=#true {
        table {
            column "Time" type="string" required=#true
            column "Event" type="string" required=#true
            column "Actor" type="user"
        }
    }
    section "Root Cause" required=#true
    section "Action Items" {
        table {
            column "Action" type="string" required=#true
            column "Owner" type="user" required=#true
            column "Status" type="string"
        }
    }
}

type "gov" description="Governance Document" folder="docs/governance" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true default="draft" {
        values "draft" "active" "retired"
    }
    field "author" type="user" required=#true
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"

    section "Policy" required=#true
    section "Scope" required=#true
    section "Enforcement"
}

type "opp" description="Opportunity" folder="docs/opportunities" {
    field "title" type="string" required=#true
    field "status" type="enum" required=#true default="identified" {
        values "identified" "evaluating" "pursuing" "completed" "declined"
    }
    field "author" type="user" required=#true
    field "date" type="string" required=#true pattern="^\\d{4}-\\d{2}-\\d{2}$" default="$TODAY"

    section "Description" required=#true
    section "Impact"
    section "Risks"
}
"#
    .to_string()
}

fn users_template() -> String {
    r##"# md-db user/team config
# See: https://github.com/decisiongraph/md-db-rs

users:
  # example:
  #   name: Example User
  #   email: user@example.com
  #   teams: [engineering]

teams:
  # engineering:
  #   name: Engineering
  #   slack: "#engineering"
"##
    .to_string()
}
