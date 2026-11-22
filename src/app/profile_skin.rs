use crate::{
    account::skin::{self, Error},
    http::Https,
    message::Message,
    ui::art,
};
use iced::{Task, task, widget::image::Handle};

#[derive(Default)]
pub(crate) struct Preview {
    https: Https,
    name: Option<String>,
    generation: u64,
    request: Option<task::Handle>,
    result: Option<Result<Handle, Error>>,
}

impl Preview {
    pub(crate) fn sync(&mut self, visible: bool, name: Option<&str>) -> Option<Task<Message>> {
        let same_name = self
            .name
            .as_deref()
            .zip(name)
            .is_some_and(|(loaded, selected)| loaded.eq_ignore_ascii_case(selected));
        if !same_name {
            self.reset();
        }
        if !visible {
            if self.request.is_some() || matches!(self.result, Some(Err(Error::Unavailable))) {
                self.reset();
            }
            return None;
        }
        if same_name {
            return None;
        }
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
                Ok(art::skin::portrait(&skin))
            },
            move |result| Message::ProfileSkinReady(generation, result),
        )
        .abortable();
        self.request = Some(handle.abort_on_drop());
        Some(task)
    }

    pub(crate) fn complete(&mut self, generation: u64, result: Result<Handle, Error>) {
        if generation == self.generation && self.request.take().is_some() {
            self.result = Some(result);
        }
    }

    fn reset(&mut self) {
        self.request = None;
        self.name = None;
        self.result = None;
    }

    pub(crate) fn image(&self) -> Option<&Handle> {
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
        let _initial = preview.sync(true, Some("FirstPlayer"));
        let initial = preview.generation;
        let image = || Handle::from_rgba(1, 1, vec![255, 255, 255, 255]);
        preview.complete(initial, Ok(image()));
        assert!(preview.image().is_some());

        let _cached = preview.sync(true, Some("FIRSTPLAYER"));
        assert_eq!(preview.generation, initial);
        assert!(preview.image().is_some());

        let _changed = preview.sync(true, Some("SecondPlayer"));
        let changed = preview.generation;
        assert_ne!(changed, initial);
        assert!(preview.image().is_none());
        preview.complete(initial, Ok(image()));
        assert_eq!(preview.status(), Some("Loading skin…"));

        let _closed = preview.sync(false, Some("SecondPlayer"));
        preview.complete(changed, Ok(image()));
        assert!(preview.image().is_none());
        let _reopened = preview.sync(true, Some("SecondPlayer"));
        preview.complete(preview.generation, Err(Error::Missing));
        assert_eq!(preview.status(), Some("No public skin for this name."));
    }
}
