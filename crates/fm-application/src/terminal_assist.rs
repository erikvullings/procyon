//! Terminal assistance (task 0333, phase 1): one-shot answers about the embedded terminal, through
//! the chat role (0322). Nothing is saved, the transcript is sent only when the person asks, and
//! the model answers in text only. A suggested command is shown to the person; it is never run.

use fm_transport_dto::{
    ChatFailureDto, TerminalAssistKindDto, TerminalAssistReplyDto, TerminalAssistRequestDto,
};
use tokio_util::sync::CancellationToken;

use crate::chat::ChatGenerator;
use crate::file_assistant::truncate_to_bytes;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole};

/// Longest natural-language request, in characters.
const MAX_REQUEST_CHARS: usize = 2 * 1024;
/// Most transcript bytes sent; the newest output is kept.
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
/// Longest working-directory or shell name, in characters.
const MAX_CONTEXT_CHARS: usize = 512;
/// Longest answer kept, in bytes.
const MAX_REPLY_BYTES: usize = 8 * 1024;
/// Upper bound on generated tokens for one answer.
const MAX_REPLY_TOKENS: u32 = 1024;

const COMMAND_PROMPT: &str = "You turn a request into one command for the person's shell. Reply \
     with exactly one line containing only the command: no explanation, no markdown, no code \
     fences. Prefer read-only commands when they satisfy the request, and never chain several \
     destructive steps.";
const EXPLAIN_PROMPT: &str = "You explain recent terminal output for the person. Say what happened \
     in a few short sentences, and whether it is an error. Use only what the output shows, and say \
     when it does not tell you enough.";
const FIX_PROMPT: &str = "The recent terminal output ends with a failed command. Explain the most \
     likely cause in one or two sentences. Then, on its own final line, write `Command: ` followed \
     by one corrected shell command. If no safe command is clear, leave out that line.";

/// Answers one terminal assistance request through the chat role.
pub(crate) async fn reply(
    generator: &impl ChatGenerator,
    request: TerminalAssistRequestDto,
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    let (system_prompt, user_prompt) = prompts(&request)?;
    let cancellation = CancellationToken::new();
    let raw = generator
        .generate_for_role(
            LlmRole::Chat,
            LlmChatGeneration {
                system_prompt,
                user_prompt,
                images: Vec::new(),
                maximum_tokens: MAX_REPLY_TOKENS,
                context_window: None,
                temperature: None,
            },
            &cancellation,
            &mut |_: &str| {},
        )
        .await
        .map_err(failure)?;
    let text = match request.kind {
        TerminalAssistKindDto::Command => first_command_line(&raw),
        TerminalAssistKindDto::Explain | TerminalAssistKindDto::Fix => {
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| truncate_to_bytes(trimmed, MAX_REPLY_BYTES).to_owned())
        }
    }
    .ok_or(ChatFailureDto::Empty)?;
    Ok(TerminalAssistReplyDto { text })
}

fn prompts(request: &TerminalAssistRequestDto) -> Result<(String, String), ChatFailureDto> {
    let context = format!(
        "Shell: {}\nWorking directory: {}",
        bounded_line(request.shell.as_deref().unwrap_or("unknown")),
        bounded_line(&request.working_directory),
    );
    match request.kind {
        TerminalAssistKindDto::Command => {
            let wanted = request
                .request
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty() && text.chars().count() <= MAX_REQUEST_CHARS)
                .ok_or(ChatFailureDto::InvalidRequest)?;
            Ok((
                COMMAND_PROMPT.to_owned(),
                format!("{context}\nRequest: {wanted}"),
            ))
        }
        TerminalAssistKindDto::Explain | TerminalAssistKindDto::Fix => {
            let transcript = request.transcript.trim();
            if transcript.is_empty() {
                return Err(ChatFailureDto::InvalidRequest);
            }
            let system = match request.kind {
                TerminalAssistKindDto::Fix => FIX_PROMPT,
                _ => EXPLAIN_PROMPT,
            };
            Ok((
                system.to_owned(),
                format!(
                    "{context}\nTerminal output:\n{}",
                    newest_bytes(transcript, MAX_TRANSCRIPT_BYTES)
                ),
            ))
        }
    }
}

/// Strips control characters and caps the length of text that is not the person's own message.
fn bounded_line(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_CONTEXT_CHARS)
        .collect()
}

/// The last `max_bytes` of `text`, starting on a character boundary.
fn newest_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The first non-empty line of a command answer, without code fences, prompts or backticks.
fn first_command_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(|line| {
            let line = line.strip_prefix("$ ").unwrap_or(line);
            line.trim().trim_matches('`').trim().to_owned()
        })
        .find(|line| !line.is_empty())
        .map(|line| truncate_to_bytes(&line, MAX_REPLY_BYTES).to_owned())
}

fn failure(error: LlmProfileError) -> ChatFailureDto {
    match error {
        LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
        LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
        LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
        _ => ChatFailureDto::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;

    use super::*;

    enum Outcome {
        Text(&'static str),
        Consent,
        NoRole,
    }

    struct FakeGenerator {
        outcome: Outcome,
        seen: StdMutex<Vec<(LlmRole, LlmChatGeneration)>>,
    }

    #[async_trait]
    impl ChatGenerator for FakeGenerator {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(false)
        }

        async fn generate_for_role(
            &self,
            role: LlmRole,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
            _on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push((role, generation));
            match self.outcome {
                Outcome::Text(text) => Ok(text.to_owned()),
                Outcome::Consent => {
                    Err(LlmProfileError::ConsentRequired("api.example.test".into()))
                }
                Outcome::NoRole => Err(LlmProfileError::RoleNotAssigned),
            }
        }
    }

    fn generator(outcome: Outcome) -> FakeGenerator {
        FakeGenerator {
            outcome,
            seen: StdMutex::new(Vec::new()),
        }
    }

    fn command(request: &str) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind: TerminalAssistKindDto::Command,
            transcript: String::new(),
            request: Some(request.to_owned()),
            working_directory: "file:///projects/app".to_owned(),
            shell: Some("zsh".to_owned()),
        }
    }

    fn explain(transcript: &str, kind: TerminalAssistKindDto) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind,
            transcript: transcript.to_owned(),
            request: None,
            working_directory: "file:///projects/app".to_owned(),
            shell: None,
        }
    }

    #[tokio::test]
    async fn a_command_answer_is_one_clean_line_asked_through_the_chat_role() {
        let fake = generator(Outcome::Text("```sh\n$ ls -la\n```"));
        let reply = reply(&fake, command("list files")).await.unwrap();

        assert_eq!(reply.text, "ls -la");
        let seen = fake.seen.lock().unwrap();
        let (role, generation) = &seen[0];
        assert_eq!(*role, LlmRole::Chat);
        assert!(generation.images.is_empty());
        assert!(generation.user_prompt.contains("Shell: zsh"));
        assert!(
            generation
                .user_prompt
                .contains("Working directory: file:///projects/app")
        );
        assert!(generation.user_prompt.ends_with("Request: list files"));
    }

    #[tokio::test]
    async fn a_command_without_a_request_never_reaches_the_model() {
        let fake = generator(Outcome::Text("ls"));
        let mut request = command("");
        request.request = None;

        assert_eq!(
            reply(&fake, request).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_explanation_keeps_the_newest_output_when_the_transcript_is_long() {
        let transcript = format!("{}END", "a".repeat(MAX_TRANSCRIPT_BYTES * 2));
        let fake = generator(Outcome::Text("It failed."));

        let reply = reply(&fake, explain(&transcript, TerminalAssistKindDto::Explain))
            .await
            .unwrap();

        assert_eq!(reply.text, "It failed.");
        let seen = fake.seen.lock().unwrap();
        let prompt = &seen[0].1.user_prompt;
        assert!(prompt.ends_with("END"));
        assert!(prompt.len() < MAX_TRANSCRIPT_BYTES + 512);
    }

    #[tokio::test]
    async fn explaining_needs_terminal_output() {
        let fake = generator(Outcome::Text("unused"));

        assert_eq!(
            reply(&fake, explain("   \n", TerminalAssistKindDto::Fix)).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_fix_keeps_the_explanation_and_its_command_line() {
        let answer = "The path does not exist.\nCommand: mkdir -p build";
        let fake = generator(Outcome::Text(answer));

        let reply = reply(
            &fake,
            explain("make: build: No such file", TerminalAssistKindDto::Fix),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, answer);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].1.system_prompt, FIX_PROMPT);
    }

    #[tokio::test]
    async fn an_empty_answer_is_an_empty_failure() {
        let blank = generator(Outcome::Text("   "));
        assert_eq!(
            reply(&blank, command("anything")).await,
            Err(ChatFailureDto::Empty)
        );

        let fences_only = generator(Outcome::Text("```\n```"));
        assert_eq!(
            reply(&fences_only, command("anything")).await,
            Err(ChatFailureDto::Empty)
        );
    }

    #[tokio::test]
    async fn consent_and_role_problems_are_reported_as_such() {
        assert_eq!(
            reply(&generator(Outcome::Consent), command("x")).await,
            Err(ChatFailureDto::ConsentRequired)
        );
        assert_eq!(
            reply(&generator(Outcome::NoRole), command("x")).await,
            Err(ChatFailureDto::RoleNotAssigned)
        );
    }
}
