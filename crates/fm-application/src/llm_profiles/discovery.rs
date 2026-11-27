//! First-run discovery uses only predefined loopback model-list requests.

use super::*;
use fm_transport_dto::{DetectedLlmServerDto, LlmDiscoveryChoiceDto, LlmDiscoveryStatusDto};

const PROBE_TIMEOUT: Duration = Duration::from_millis(450);

#[derive(Clone, Copy)]
struct DiscoveryPreset<'a> {
    kind: LlmPreset,
    name: &'a str,
    base_url: &'a str,
    path: &'a str,
    os: Option<&'a str>,
    list_field: &'a str,
    model_field: &'a str,
}

const PRESETS: &[DiscoveryPreset<'static>] = &[
    DiscoveryPreset {
        kind: LlmPreset::Ollama,
        name: "Ollama",
        base_url: "http://127.0.0.1:11434",
        path: "/api/tags",
        os: None,
        list_field: "models",
        model_field: "name",
    },
    DiscoveryPreset {
        kind: LlmPreset::LmStudio,
        name: "LM Studio",
        base_url: "http://127.0.0.1:1234",
        path: "/v1/models",
        os: None,
        list_field: "data",
        model_field: "id",
    },
    DiscoveryPreset {
        kind: LlmPreset::Omlx,
        name: "oMLX",
        base_url: "http://127.0.0.1:8000",
        path: "/v1/models",
        os: Some("macos"),
        list_field: "data",
        model_field: "id",
    },
];

pub(super) fn preset_defaults(kind: LlmPreset) -> Option<(LlmPreset, &'static str, &'static str)> {
    PRESETS
        .iter()
        .find(|preset| preset.kind == kind)
        .map(|preset| (preset.kind, preset.name, preset.base_url))
}

impl DiscoveryPreset<'_> {
    fn probe_url(self, os: &str, policy: &LlmHostPolicy) -> Option<Url> {
        if self.os.is_some_and(|expected| expected != os) {
            return None;
        }
        let endpoint = normalize_endpoint(self.base_url).ok()?;
        let mut url = Url::parse(&endpoint.base_url).ok()?;
        // Numeric loopback addresses avoid DNS and hosts-file resolution altogether.
        if !matches!(url.host(), Some(Host::Ipv4(_) | Host::Ipv6(_)))
            || !endpoint.loopback
            || !policy.permits(&endpoint)
            || !self.path.starts_with('/')
        {
            return None;
        }
        url.set_path(self.path);
        Some(url)
    }

    fn result(self, body: &[u8]) -> Option<DetectedLlmServerDto> {
        let value: Value = serde_json::from_slice(body).ok()?;
        let models = bounded_model_ids(
            value
                .get(self.list_field)?
                .as_array()?
                .iter()
                .filter_map(|entry| entry.get(self.model_field)?.as_str())
                .map(str::to_owned)
                .collect(),
        );
        Some(DetectedLlmServerDto {
            preset: preset_to_dto(self.kind),
            name: self.name.to_owned(),
            base_url: self.base_url.to_owned(),
            models,
        })
    }
}

async fn detect(
    presets: &[DiscoveryPreset<'_>],
    os: &str,
    policy: &LlmHostPolicy,
) -> Result<Vec<DetectedLlmServerDto>, LlmProfileError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(map_reqwest)?;
    let results = futures::future::join_all(presets.iter().map(|preset| async {
        let url = preset.probe_url(os, policy)?;
        let response = client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = read_bounded_body(response, &CancellationToken::new())
            .await
            .ok()?;
        preset.result(&body)
    }))
    .await;
    Ok(results.into_iter().flatten().collect())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DiscoveryState {
    #[default]
    Pending,
    Skipped,
    Complete,
}

impl LlmProfileService {
    pub(crate) fn discovery_status(&self) -> Result<LlmDiscoveryStatusDto, LlmProfileError> {
        let document = self.lock()?;
        Ok(LlmDiscoveryStatusDto {
            loopback_available: self.host_policy.allow_loopback,
            first_run_pending: document.discovery == DiscoveryState::Pending
                && document.profiles.is_empty(),
            show_banner: document.discovery == DiscoveryState::Skipped
                && document.profiles.is_empty(),
        })
    }

    pub(crate) async fn detect_local_servers(
        &self,
    ) -> Result<Vec<DetectedLlmServerDto>, LlmProfileError> {
        detect(PRESETS, std::env::consts::OS, &self.host_policy).await
    }

    pub(crate) fn choose_discovery(
        &self,
        choice: LlmDiscoveryChoiceDto,
    ) -> Result<(), LlmProfileError> {
        let mut document = self.lock()?;
        let mut next = document.clone();
        next.discovery = match choice {
            LlmDiscoveryChoiceDto::Later => DiscoveryState::Skipped,
            LlmDiscoveryChoiceDto::Complete => DiscoveryState::Complete,
            LlmDiscoveryChoiceDto::Reset => DiscoveryState::Pending,
            LlmDiscoveryChoiceDto::Use { preset, model } => {
                self.assign_discovered_server(&mut next, preset, model)?;
                DiscoveryState::Complete
            }
        };
        self.persist(&next)?;
        *document = next;
        Ok(())
    }

    fn assign_discovered_server(
        &self,
        document: &mut LlmProfilesDocument,
        kind: LlmPresetDto,
        model: String,
    ) -> Result<(), LlmProfileError> {
        let preset = PRESETS
            .iter()
            .find(|preset| preset.kind == preset_from_dto(kind))
            .ok_or(LlmProfileError::InvalidConfiguration)?;
        if preset
            .probe_url(std::env::consts::OS, &self.host_policy)
            .is_none()
        {
            return Err(LlmProfileError::PolicyDenied);
        }
        if model.len() > 512 {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let mut draft = preset_draft(preset.kind, preset.name, preset.base_url);
        draft.model = model;
        let profile = profile_from_draft(Uuid::new_v4(), draft, None);
        validate_profile(&profile)?;
        let profile_id = match document.profiles.iter().find(|saved| {
            saved.preset == profile.preset
                && saved.base_url == profile.base_url
                && saved.model == profile.model
                && saved.credential_ref.is_none()
                && saved
                    .capabilities
                    .contains(&LlmApiCapability::ChatCompletions)
        }) {
            Some(saved) => saved.id,
            None => {
                let id = profile.id;
                document.profiles.push(profile.clone());
                id
            }
        };
        document.active_profile_id.get_or_insert(profile_id);
        let assignment = RoleAssignment {
            profile_id,
            model: profile.model,
            allow_cloud: false,
            thinking: document
                .roles
                .embeddings
                .as_ref()
                .map_or_else(ThinkingEffort::default, |role| role.thinking),
        };
        document.roles.chat = Some(RoleAssignment {
            thinking: document
                .roles
                .chat
                .as_ref()
                .map_or_else(ThinkingEffort::default, |role| role.thinking),
            ..assignment.clone()
        });
        document.roles.embeddings = Some(assignment);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_credentials::InMemoryCredentialStore;
    use std::net::Ipv4Addr;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    #[test]
    fn built_in_presets_are_unique_and_only_numeric_loopback_is_eligible() {
        let policy = LlmHostPolicy::desktop();
        let mut kinds = BTreeSet::new();
        for preset in PRESETS {
            assert!(kinds.insert(preset.kind));
            assert!(preset.probe_url("macos", &policy).is_some());
            assert_eq!(
                preset.probe_url("linux", &policy).is_some(),
                preset.kind != LlmPreset::Omlx
            );
            assert!(
                preset
                    .probe_url("macos", &LlmHostPolicy::server([]))
                    .is_none()
            );
        }
        for base_url in [
            "https://example.com",
            "http://192.168.1.1",
            "http://localhost",
        ] {
            let preset = DiscoveryPreset {
                base_url,
                ..PRESETS[0]
            };
            assert!(preset.probe_url("macos", &policy).is_none());
        }
    }

    #[tokio::test]
    async fn detects_a_fixture_preset_by_its_declared_path_and_model_fields() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = DiscoveryPreset {
            kind: LlmPreset::Vllm,
            name: "Fixture AI",
            base_url: &base_url,
            path: "/fixture/models",
            os: None,
            list_field: "catalogue",
            model_field: "key",
        };
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, "GET /fixture/models HTTP/1.1\r\n");
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let body =
                r#"{"catalogue":[{"key":"fixture-model"},{"key":"fixture-model"},{"key":""}]}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let found = detect(&[fixture], "fixture-os", &LlmHostPolicy::desktop())
            .await
            .unwrap();
        upstream.await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Fixture AI");
        assert_eq!(found[0].base_url, base_url);
        assert_eq!(found[0].preset, LlmPresetDto::Vllm);
        assert_eq!(found[0].models, ["fixture-model"]);
        assert!(fixture.result(b"{}").is_none());
        assert!(
            fixture
                .result(br#"{"catalogue":[]}"#)
                .unwrap()
                .models
                .is_empty()
        );
    }

    #[test]
    fn first_run_state_persists_and_local_setup_binds_both_roles_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let open = || {
            LlmProfileService::new(
                SettingsStore::new(directory.path()),
                Arc::new(InMemoryCredentialStore::new()),
                Arc::new(ReqwestLlmProbeTransport::new()),
                LlmHostPolicy::desktop(),
            )
            .unwrap()
        };
        let service = open();
        assert!(service.discovery_status().unwrap().first_run_pending);
        service
            .choose_discovery(LlmDiscoveryChoiceDto::Later)
            .unwrap();
        let service = open();
        let skipped = service.discovery_status().unwrap();
        assert!(!skipped.first_run_pending);
        assert!(skipped.show_banner);
        service
            .choose_discovery(LlmDiscoveryChoiceDto::Complete)
            .unwrap();
        assert!(!open().discovery_status().unwrap().show_banner);
        service
            .choose_discovery(LlmDiscoveryChoiceDto::Reset)
            .unwrap();
        assert!(open().discovery_status().unwrap().first_run_pending);

        assert_eq!(
            service.choose_discovery(LlmDiscoveryChoiceDto::Use {
                preset: LlmPresetDto::OpenAiCompatible,
                model: "cloud".into(),
            }),
            Err(LlmProfileError::InvalidConfiguration)
        );
        assert!(open().discovery_status().unwrap().first_run_pending);
        let choose = LlmDiscoveryChoiceDto::Use {
            preset: LlmPresetDto::Ollama,
            model: "local-model".into(),
        };
        service.choose_discovery(choose.clone()).unwrap();
        service.choose_discovery(choose).unwrap();
        let service = open();
        assert!(!service.discovery_status().unwrap().first_run_pending);
        let profiles = service.list().unwrap();
        assert_eq!(profiles.len(), 1);
        let roles = service.roles().unwrap();
        for role in [roles.chat.unwrap(), roles.embeddings.unwrap()] {
            assert_eq!(role.profile_id, profiles[0].id);
            assert_eq!(role.model, "local-model");
            assert!(!role.allow_cloud);
        }
    }
}
