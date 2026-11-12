# QA results: Ram Code providers

| Case | Verdict | Evidence |
|---|---|---|
| Q18 | pass | Ram Code writes to disk in three places. `src/providers.rs` `Providers::save_active` writes `providers.json` with the `active` name only. `src/app.rs` `start_task_with_text` and `src/main.rs` `run_worker` write the message history, which holds no credential. `src/main.rs` `run_worker` writes events: the trace has the provider name, base URL and `Credential::label`, never the key. The key reaches only the request headers in `auth_headers` and `OpenAiLlm::complete`. |
