use crate::error::{Error, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(name = "spr")]
#[clap(
    about = "Submit pull requests for individual, amendable, rebaseable commits to GitHub"
)]
pub struct Cli {
    /// Change to DIR before performing any operations
    #[clap(long, value_name = "DIR")]
    cd: Option<String>,

    /// GitHub personal access token (if not given taken from git config
    /// spr.githubAuthToken)
    #[clap(long)]
    github_auth_token: Option<String>,

    /// GitHub repository ('org/name', if not given taken from config
    /// spr.githubRepository)
    #[clap(long)]
    github_repository: Option<String>,

    /// prefix to be used for branches created for pull requests (if not given
    /// taken from git config spr.branchPrefix, defaulting to
    /// 'spr/<GITHUB_USERNAME>/')
    #[clap(long)]
    branch_prefix: Option<String>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Interactive assistant for configuring spr in a local GitHub-backed Git
    /// repository
    Init,

    /// Create a new or update an existing Pull Request on GitHub from the
    /// current HEAD commit
    Diff(crate::commands::diff::DiffOptions),

    /// Reformat commit message
    Format(crate::commands::format::FormatOptions),

    /// Land a reviewed Pull Request
    Land(crate::commands::land::LandOptions),

    /// Update local commit message with content on GitHub
    Amend(crate::commands::amend::AmendOptions),
}

#[derive(Debug, thiserror::Error)]
pub enum OptionsError {
    #[error(
        "GitHub repository must be given as 'OWNER/REPO', but given value was '{0}'"
    )]
    InvalidRepository(String),
}

pub fn spr() -> Result<()> {
    let cli = Cli::parse();

    if let Some(path) = &cli.cd {
        if let Err(err) = std::env::set_current_dir(&path) {
            eprintln!("Could not change directory to {:?}", &path);
            return Err(err.into());
        }
    }

    if let Commands::Init = cli.command {
        return crate::executor::run(async move {
            crate::commands::init::init(&cli).await
        });
    }

    let repo = git2::Repository::discover(std::env::current_dir()?)?;

    let config = repo.config()?;

    let github_auth_token = match cli.github_auth_token {
        Some(v) => Ok(v),
        None => config.get_string("spr.githubAuthToken"),
    }?;

    let github_repository = match cli.github_repository {
        Some(v) => Ok(v),
        None => config.get_string("spr.githubRepository"),
    }?;

    let (github_owner, github_repo) = {
        let captures = lazy_regex::regex!(r#"^([\w\-\.]+)/([\w\-\.]+)$"#)
            .captures(&github_repository)
            .ok_or_else(|| {
                OptionsError::InvalidRepository(github_repository.clone())
            })?;
        (
            captures.get(1).unwrap().as_str().to_string(),
            captures.get(2).unwrap().as_str().to_string(),
        )
    };

    let github_remote_name = config
        .get_string("spr.githubRemoteName")
        .unwrap_or_else(|_| "origin".to_string());
    let github_master_branch = config
        .get_string("spr.githubMasterBranch")
        .unwrap_or_else(|_| "master".to_string());
    let branch_prefix = config.get_string("spr.branchPrefix")?;

    let config = crate::config::Config::new(
        github_owner,
        github_repo,
        github_remote_name,
        github_master_branch,
        branch_prefix,
    );

    octocrab::initialise(
        octocrab::Octocrab::builder().personal_token(github_auth_token),
    )?;

    crate::executor::run(async move {
        let git = crate::git::Git::new(repo);
        let mut gh = crate::github::GitHub::new(config.clone());

        match cli.command {
            Commands::Diff(opts) => {
                crate::commands::diff::diff(opts, &git, &mut gh, &config)
                    .await?
            }
            Commands::Land(opts) => {
                crate::commands::land::land(opts, &git, &mut gh, &config)
                    .await?
            }
            Commands::Amend(opts) => {
                crate::commands::amend::amend(opts, &git, &mut gh, &config)
                    .await?
            }
            Commands::Format(opts) => {
                crate::commands::format::format(opts, &git, &config).await?
            }
            _ => (),
        };

        Ok::<_, Error>(())
    })
}