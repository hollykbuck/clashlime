use crate::core;

impl crate::app::App {
    pub(crate) async fn select_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        self.say(format!("Validating profile {uid}…"));
        let mut candidate = self.profiles.clone();
        candidate.current = Some(uid.clone());
        match core::CoreManager::new()
            .validate_only(&self.config, &candidate)
            .await
        {
            Ok(()) => match candidate.save() {
                Ok(()) => match core::request_restart().await {
                    Ok(()) => {
                        self.profiles = candidate;
                        self.say(format!("Profile {uid} activated"));
                    }
                    Err(error) => {
                        self.say(format!(
                            "Profile was valid but could not be activated: {error}"
                        ));
                    }
                },
                Err(error) => self.say(format!(
                    "Profile was valid but could not be activated: {error}"
                )),
            },
            Err(error) => self.say(format!("Profile rejected; current core kept: {error}")),
        }
        self.refresh().await;
    }

    pub(crate) async fn update_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match self.profiles.update_validated(&uid, &self.config).await {
            Ok(()) => self.say(format!("Profile {uid} updated")),
            Err(error) => self.say(format!("Update failed: {error}")),
        }
    }

    pub(crate) async fn delete_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match self.profiles.delete(&uid) {
            Ok(()) => self.say(format!("Profile {uid} deleted")),
            Err(error) => self.say(format!("Delete failed: {error}")),
        }
    }
}
