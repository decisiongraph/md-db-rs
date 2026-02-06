use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::error::{Error, Result};

/// Top-level user/team configuration loaded from YAML.
#[derive(Debug, Clone)]
pub struct UserConfig {
    pub users: HashMap<String, UserDef>,
    pub teams: HashMap<String, TeamDef>,
}

/// A user definition with handle and arbitrary extra attributes.
#[derive(Debug, Clone)]
pub struct UserDef {
    pub handle: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub teams: Vec<String>,
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// A team definition with name, member teams, and arbitrary extra attributes.
#[derive(Debug, Clone)]
pub struct TeamDef {
    pub id: String,
    pub name: Option<String>,
    pub teams: Vec<String>,
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl UserConfig {
    /// Load user/team config from a YAML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::FileNotFound(path.to_path_buf()));
        }
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parse user/team config from a YAML string.
    pub fn from_str(content: &str) -> Result<Self> {
        let raw: serde_yaml::Value = serde_yaml::from_str(content)
            .map_err(|e| Error::FrontmatterParse(format!("user config: {e}")))?;

        let mut users = HashMap::new();
        let mut teams = HashMap::new();

        // Parse users
        if let Some(users_map) = raw.get("users").and_then(|v| v.as_mapping()) {
            for (key, val) in users_map {
                let handle = key
                    .as_str()
                    .ok_or_else(|| Error::FrontmatterParse("user key must be string".into()))?
                    .to_string();

                let user = parse_user_def(&handle, val)?;
                users.insert(handle, user);
            }
        }

        // Parse teams
        if let Some(teams_map) = raw.get("teams").and_then(|v| v.as_mapping()) {
            for (key, val) in teams_map {
                let id = key
                    .as_str()
                    .ok_or_else(|| Error::FrontmatterParse("team key must be string".into()))?
                    .to_string();

                let team = parse_team_def(&id, val)?;
                teams.insert(id, team);
            }
        }

        Ok(Self { users, teams })
    }

    /// Check if a `@handle` reference is valid (user or team).
    /// Accepts: `@handle` for users, `@team/name` for teams.
    pub fn is_valid_ref(&self, reference: &str) -> bool {
        if let Some(stripped) = reference.strip_prefix('@') {
            if let Some(team_name) = stripped.strip_prefix("team/") {
                self.teams.contains_key(team_name)
            } else {
                self.users.contains_key(stripped)
            }
        } else {
            false
        }
    }

    /// Check if a reference is a valid user (not team).
    pub fn is_valid_user(&self, reference: &str) -> bool {
        if let Some(stripped) = reference.strip_prefix('@') {
            if stripped.starts_with("team/") {
                false
            } else {
                self.users.contains_key(stripped)
            }
        } else {
            false
        }
    }

    /// Get all user handles as `@handle`.
    pub fn all_user_handles(&self) -> Vec<String> {
        self.users.keys().map(|h| format!("@{h}")).collect()
    }

    /// Get all team names as `@team/name`.
    pub fn all_team_names(&self) -> Vec<String> {
        self.teams.keys().map(|t| format!("@team/{t}")).collect()
    }

    /// Recursively expand all members of a team (users + nested team members).
    /// Returns user handles (without @).
    pub fn expand_team_members(&self, team_id: &str) -> HashSet<String> {
        let mut members = HashSet::new();
        let mut visited = HashSet::new();
        self.expand_team_recursive(team_id, &mut members, &mut visited);
        members
    }

    fn expand_team_recursive(
        &self,
        team_id: &str,
        members: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(team_id.to_string()) {
            return; // prevent cycles
        }

        // Add direct user members
        for (handle, user) in &self.users {
            if user.teams.contains(&team_id.to_string()) {
                members.insert(handle.clone());
            }
        }

        // Recurse into sub-teams
        if let Some(team) = self.teams.get(team_id) {
            for sub_team in &team.teams {
                self.expand_team_recursive(sub_team, members, visited);
            }
        }
    }
}

fn parse_user_def(handle: &str, val: &serde_yaml::Value) -> Result<UserDef> {
    let mapping = val
        .as_mapping()
        .ok_or_else(|| Error::FrontmatterParse(format!("user '{handle}' must be a mapping")))?;

    let name = mapping
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let email = mapping
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let teams = mapping
        .get("teams")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut extra = BTreeMap::new();
    for (k, v) in mapping {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !matches!(key.as_str(), "name" | "email" | "teams") {
            extra.insert(key, v.clone());
        }
    }

    Ok(UserDef {
        handle: handle.to_string(),
        name,
        email,
        teams,
        extra,
    })
}

fn parse_team_def(id: &str, val: &serde_yaml::Value) -> Result<TeamDef> {
    let mapping = val
        .as_mapping()
        .ok_or_else(|| Error::FrontmatterParse(format!("team '{id}' must be a mapping")))?;

    let name = mapping
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let teams = mapping
        .get("teams")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut extra = BTreeMap::new();
    for (k, v) in mapping {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !matches!(key.as_str(), "name" | "teams") {
            extra.insert(key, v.clone());
        }
    }

    Ok(TeamDef {
        id: id.to_string(),
        name,
        teams,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> UserConfig {
        UserConfig::from_str(
            r##"
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
    slack: "#security"

  leadership:
    name: Leadership

  engineering:
    name: Engineering
    teams: [platform, security]
"##,
        )
        .unwrap()
    }

    #[test]
    fn test_load_users() {
        let config = test_config();
        assert_eq!(config.users.len(), 3);
        assert_eq!(config.teams.len(), 4);

        let onni = &config.users["onni"];
        assert_eq!(onni.name.as_deref(), Some("Onni Hakala"));
        assert_eq!(onni.email.as_deref(), Some("onni@flaky.build"));
        assert!(onni.teams.contains(&"platform".to_string()));
        assert_eq!(onni.extra["role"].as_str(), Some("staff-engineer"));
    }

    #[test]
    fn test_load_teams() {
        let config = test_config();
        let platform = &config.teams["platform"];
        assert_eq!(platform.name.as_deref(), Some("Platform Team"));
        assert_eq!(platform.extra["slack"].as_str(), Some("#platform"));
        assert_eq!(platform.extra["lead"].as_str(), Some("onni"));

        let eng = &config.teams["engineering"];
        assert!(eng.teams.contains(&"platform".to_string()));
        assert!(eng.teams.contains(&"security".to_string()));
    }

    #[test]
    fn test_valid_refs() {
        let config = test_config();
        assert!(config.is_valid_ref("@onni"));
        assert!(config.is_valid_ref("@alice"));
        assert!(config.is_valid_ref("@team/platform"));
        assert!(config.is_valid_ref("@team/engineering"));
        assert!(!config.is_valid_ref("@unknown"));
        assert!(!config.is_valid_ref("@team/unknown"));
        assert!(!config.is_valid_ref("onni")); // missing @
    }

    #[test]
    fn test_is_valid_user() {
        let config = test_config();
        assert!(config.is_valid_user("@onni"));
        assert!(!config.is_valid_user("@team/platform"));
        assert!(!config.is_valid_user("@unknown"));
    }

    #[test]
    fn test_expand_team_members() {
        let config = test_config();

        // Platform team: onni + alice
        let platform = config.expand_team_members("platform");
        assert!(platform.contains("onni"));
        assert!(platform.contains("alice"));
        assert!(!platform.contains("bob"));

        // Engineering team: platform(onni, alice) + security(bob)
        let eng = config.expand_team_members("engineering");
        assert!(eng.contains("onni"));
        assert!(eng.contains("alice"));
        assert!(eng.contains("bob"));
    }

    #[test]
    fn test_cycle_protection() {
        // Team A contains B, B contains A
        let config = UserConfig::from_str(
            r#"
users:
  x:
    name: X
    teams: [a]
teams:
  a:
    name: A
    teams: [b]
  b:
    name: B
    teams: [a]
"#,
        )
        .unwrap();

        // Should not infinite loop
        let members = config.expand_team_members("a");
        assert!(members.contains("x"));
    }

    #[test]
    fn test_all_handles_and_names() {
        let config = test_config();
        let handles = config.all_user_handles();
        assert!(handles.contains(&"@onni".to_string()));
        assert!(handles.contains(&"@alice".to_string()));

        let teams = config.all_team_names();
        assert!(teams.contains(&"@team/platform".to_string()));
        assert!(teams.contains(&"@team/engineering".to_string()));
    }
}
