use crate::OpenDocApp;

impl OpenDocApp {
    pub(crate) fn invalidate_source_state(&mut self) {
        self.signatures.clear();
        self.invalidate_projection();
    }

    pub(crate) fn invalidate_projection(&mut self) {
        self.saved_projection = None;
    }
}
