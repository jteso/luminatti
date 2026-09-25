use std::io::{IsTerminal, Write};

use crate::{
    config::configuration::DraftConfig, error::LuminattiError, git_entity::GitEntity,
    provider::LuminattiProvider,
};

pub struct DraftCommand {
    pub git_entity: GitEntity,
    pub context: Option<String>,
    pub draft_config: DraftConfig,
}

impl DraftCommand {
    pub async fn execute(&self, provider: &LuminattiProvider) -> Result<(), LuminattiError> {
        let result = provider.draft(self).await?;

        // Only add newline when outputting to terminal, not when piped (e.g., `luminatti draft | pbcopy`)
        if std::io::stdout().is_terminal() {
            println!("{result}");
        } else {
            print!("{result}");
        }
        std::io::stdout().flush()?;
        Ok(())
    }
}
