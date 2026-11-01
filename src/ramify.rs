use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub root: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }

    pub fn text(&self) -> String {
        let mut out = self.stdout.trim_end().to_string();
        let err = self.stderr.trim_end();
        if !err.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(err);
        }
        out
    }
}

pub trait Runner: Send + Sync {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Status,
    Step,
    Up,
    Down,
    Workflow,
    CreateWorktree(String),
}

impl Action {
    pub fn args(&self) -> Vec<String> {
        match self {
            Action::Status => vec!["status".into()],
            Action::Step => vec!["step".into()],
            Action::Up => vec!["up".into()],
            Action::Down => vec!["down".into()],
            Action::Workflow => vec!["workflow".into()],
            Action::CreateWorktree(slug) => vec!["new".into(), slug.clone()],
        }
    }
}

pub struct RealRunner {
    pub program: String,
}

impl Default for RealRunner {
    fn default() -> Self {
        Self {
            program: std::env::var("RAM_CODE_RAMIFY").unwrap_or_else(|_| "ramify".into()),
        }
    }
}

impl Runner for RealRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        let out = std::process::Command::new(&self.program)
            .args(&spec.args)
            .current_dir(&spec.root)
            .env("RAMIFY_ROOT", &spec.root)
            .output()
            .map_err(|e| format!("could not run ramify: {e}"))?;
        Ok(CommandOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}
