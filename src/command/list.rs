use crate::{
    error::LuminattiError,
    git_entity::{commit::Commit, GitEntity},
    provider::LuminattiProvider,
    vcs::VcsBackend,
};

use super::{explain::ExplainCommand, LuminattiCommand};

pub struct ListCommand;

impl ListCommand {
    pub async fn execute(
        &self,
        provider: &LuminattiProvider,
        backend: &dyn VcsBackend,
    ) -> Result<(), LuminattiError> {
        let sha = LuminattiCommand::get_sha_from_fzf(backend)?;
        let info = backend.get_commit(&sha)?;
        let git_entity = GitEntity::Commit(Commit::from_commit_info(info));
        ExplainCommand {
            git_entity,
            query: None,
        }
        .execute(provider)
        .await
    }
}
