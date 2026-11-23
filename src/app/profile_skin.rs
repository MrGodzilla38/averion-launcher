use crate::{
    account::skin::{self, Error},
    http::Https,
    message::Message,
    ui::art::skin::{Portraits, portraits},
};
use iced::{Task, task};

#[derive(Default)]
pub(crate) struct Preview {
    https: Https,
    name: Option<String>,
    generation: u64,
    request: Option<task::Handle>,
    result: Option<Result<Portraits, Error>>,
}

impl Preview {
    pub(crate) fn sync(
        &mut self,
        retry_unavailable: bool,
        name: Option<&str>,
    ) -> Option<Task<Message>> {
        let same_name = self
            .name
            .as_deref()
            .zip(name)
            .is_some_and(|(loaded, selected)| loaded.eq_ignore_ascii_case(selected));
        if same_name && !(retry_unavailable && matches!(self.result, Some(Err(Error::Unavailable))))
        {
            return None;
        }
        self.reset();
        let name = name?.to_owned();

        self.name = Some(name.clone());
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let https = self.https.clone();
        let (task, handle) = Task::perform(
            async move {
                let skin = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    skin::lookup(&https, &name),
                )
                .await
                .map_err(|_| Error::Unavailable)??;
                Ok(portraits(&skin))
            },
            move |result| Message::ProfileSkinReady(generation, result),
        )
        .abortable();
        self.request = Some(handle.abort_on_drop());
        Some(task)
    }

    pub(crate) fn complete(&mut self, generation: u64, result: Result<Portraits, Error>) {
        if generation == self.generation && self.request.take().is_some() {
            self.result = Some(result);
        }
    }

    fn reset(&mut self) {
        self.request = None;
        self.name = None;
        self.result = None;
    }

    pub(crate) fn portraits(&self) -> Option<&Portraits> {
        self.result.as_ref().and_then(|result| result.as_ref().ok())
    }

    pub(crate) fn status(&self) -> Option<&'static str> {
        match self.result {
            Some(Err(Error::Missing)) => Some("No public skin for this name."),
            Some(Err(Error::Unavailable)) => Some("Skin preview unavailable."),
            _ if self.request.is_some() => Some("Loading skin…"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_accounts_clears_the_image_and_ignores_obsolete_results() {
        let mut preview = Preview::default();
        let _initial = preview.sync(false, Some("FirstPlayer"));
        let initial = preview.generation;
        let image = || {
            let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![255; 4]);
            Portraits {
                profile: handle.clone(),
                navigation: handle,
            }
        };
        preview.complete(initial, Ok(image()));
        assert!(preview.portraits().is_some());

        let _cached = preview.sync(true, Some("FIRSTPLAYER"));
        assert_eq!(preview.generation, initial);
        assert!(preview.portraits().is_some());

        let _changed = preview.sync(false, Some("SecondPlayer"));
        let changed = preview.generation;
        assert_ne!(changed, initial);
        assert!(preview.portraits().is_none());
        preview.complete(initial, Ok(image()));
        assert_eq!(preview.status(), Some("Loading skin…"));

        assert!(preview.sync(false, Some("SecondPlayer")).is_none());
        let _removed = preview.sync(false, None);
        preview.complete(changed, Ok(image()));
        assert!(preview.portraits().is_none());
        let _reopened = preview.sync(true, Some("SecondPlayer"));
        preview.complete(preview.generation, Err(Error::Missing));
        assert_eq!(preview.status(), Some("No public skin for this name."));
    }

    #[test]
    fn opening_the_profile_retries_an_unavailable_skin() {
        let mut preview = Preview::default();
        assert!(preview.sync(false, Some("Player")).is_some());
        preview.complete(preview.generation, Err(Error::Unavailable));
        assert!(preview.sync(false, Some("Player")).is_none());
        assert!(preview.sync(true, Some("Player")).is_some());
        assert_eq!(preview.status(), Some("Loading skin…"));
    }
}
