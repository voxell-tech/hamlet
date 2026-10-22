//! An agent's dealings with the librarian: offering a skill and
//! asking which skill to use.

use std::time::Duration;

use anyhow::{Result, bail};
use hamlet_model::log;
use hamlet_rules::hamlet::Hamlet;
use hamlet_rules::library::{self, LIBRARIAN, Library};

use crate::agent::{Agent, Event, Listener};

impl Agent {
    /// Offers the skill `skill` of this agent's house to the library,
    /// which takes it or leaves it where it is. The agent that offers
    /// it pays for the librarian's reading.
    pub(super) async fn submit(
        &mut self,
        hamlet: &Hamlet,
        skill: &str,
        reason: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let Some(library) = self.tools.library().cloned() else {
            bail!("this home has no library");
        };
        let named = !skill.is_empty()
            && skill.chars().all(|c| {
                c.is_ascii_alphanumeric() || c == '-' || c == '_'
            });
        if !named {
            bail!(
                "`skill` is the name of its folder under skills/, \
                 in letters, digits and hyphens"
            );
        }
        if reason.trim().is_empty() {
            bail!("say in `reason` who else would use it");
        }
        let from = self.tools.home().skills().join(skill);
        if !from.is_dir() {
            bail!("you have no skill in skills/{skill}");
        }
        if let Some(why) = library.unfit(&from) {
            bail!("{why}");
        }
        let me = self.name();
        if let Some(why) = hamlet.stopped(&self.task) {
            bail!("{why}");
        }
        let who = self
            .resident
            .as_ref()
            .map_or(me.clone(), |resident| resident.name.clone());
        let by = format!("{who}@{}", hamlet.name());
        let asked = library.question(&from, &by, reason);
        let said =
            self.consult(hamlet, &library, &who, &asked, on).await?;
        let (taken, why) = library::verdict(&said);
        if taken {
            library.shelve(&from, &by, &why)?;
            Ok(format!(
                "The librarian took `{skill}`: {why} It is in the \
                 library now and no longer in your workspace. Use \
                 the library's, as every agent can."
            ))
        } else {
            library.refuse(skill, &by, &why)?;
            Ok(format!(
                "The librarian did not take `{skill}`: {why} It \
                 stays in your workspace."
            ))
        }
    }

    /// Asks the librarian which skill of the library does what
    /// `question` is after, and gives back what it said.
    pub(super) async fn inquire(
        &mut self,
        hamlet: &Hamlet,
        question: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let Some(library) = self.tools.library().cloned() else {
            bail!("this home has no library");
        };
        if question.trim().is_empty() {
            bail!("say in `question` what you are looking for");
        }
        let me = self.name();
        if let Some(why) = hamlet.stopped(&self.task) {
            bail!("{why}");
        }
        let who = self
            .resident
            .as_ref()
            .map_or(me, |resident| resident.name.clone());
        let by = format!("{who}@{}", hamlet.name());
        let asked = library.inquiry(question, &by);
        self.consult(hamlet, &library, &who, &asked, on).await
    }

    /// Puts `asked` to the librarian in its own house and gives back
    /// its one answer. `who` called on it and pays.
    async fn consult(
        &mut self,
        hamlet: &Hamlet,
        library: &Library,
        who: &str,
        asked: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        on(Event::Consulted {
            by: who.to_string(),
        });
        let me = self.name();
        let desk = library.desk()?;
        let path = desk
            .sessions()
            .join(format!("asked_{}.jsonl", log::now_millis()));
        let mut librarian = self.beside(desk, &path)?;
        librarian.answering = true;
        let (payer, task) = (hamlet.again(), self.task.clone());
        let mut relay = |event: Event| {
            // The librarian lives in no hamlet, so its answer is
            // charged to whoever asked for it.
            if let Event::ModelCall(call) = &event {
                let _ = payer.spend(&me, &task, call, 0);
            }
            on(Event::Agent {
                name: LIBRARIAN.to_string(),
                event: Box::new(event),
            })
        };
        // Boxed because the librarian's turn is this same code.
        tokio::time::timeout(
            Duration::from_secs(self.rules.ballot_secs),
            Box::pin(librarian.run("library", asked, &mut relay)),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("the librarian did not answer in time")
        })?
    }
}
