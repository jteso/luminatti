use spinoff::{spinners, Color, Spinner};

use crate::{error::LuminattiError, git_entity::GitEntity, provider::LuminattiProvider};

use super::LuminattiCommand;

pub struct ExplainCommand {
    pub git_entity: GitEntity,
    pub query: Option<String>,
}

impl ExplainCommand {
    pub async fn execute(&self, provider: &LuminattiProvider) -> Result<(), LuminattiError> {
        LuminattiCommand::print_with_mdcat(self.git_entity.format_static_details(provider))?;
        if let Some(query) = &self.query {
            LuminattiCommand::print_with_mdcat(format!("`query`: {query}"))?;
        }

        let spinner_text = match &self.query {
            Some(_) => "Generating answer...".to_string(),
            None => "Generating summary...".to_string(),
        };

        let mut spinner = Spinner::new(spinners::Dots, spinner_text, Color::Blue);
        let result = provider.explain(self).await?;
        spinner.success("Done");

        LuminattiCommand::print_with_mdcat(result)?;
        Ok(())
    }
}
