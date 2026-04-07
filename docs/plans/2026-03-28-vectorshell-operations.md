# VectorShell Operations Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a new reusable `vectorshell-operations` skill that helps agents operate live VectorShell sessions through the current server API using a conversation-first workflow, tool execution when needed, and artifact support.

**Architecture:** Add a new skill directory under `skill/` with one compact `SKILL.md` that defines triggering, boundaries, workflow, and reporting rules. Put detailed endpoint and flow guidance into four focused reference files so the main skill stays short, searchable, and aligned with the repository’s newer `install_id` and `session_id` model.

**Tech Stack:** Markdown skill files, repository-local docs, existing VectorShell API/SSE references, git

---

## File Map

### New files
- `skill/vectorshell-operations/SKILL.md` — main skill trigger and execution policy
- `skill/vectorshell-operations/references/api-endpoints.md` — current endpoint map using `install_id`/`session_id` aware wording
- `skill/vectorshell-operations/references/message-workflow.md` — conversation/message-first execution path
- `skill/vectorshell-operations/references/tool-artifact-workflow.md` — direct tool and artifact flow
- `skill/vectorshell-operations/references/output-format.md` — final reporting template and sanitization rules

### Existing files to read or reference
- `skill/vectorshell/SKILL.md` — outdated predecessor to mine for reusable wording only
- `skill/vectorshell/references/api-endpoints.md` — older endpoint examples to update conceptually
- `skill/vectorshell/references/sse-events.md` — older SSE reference to update conceptually
- `docs/specs/2026-03-28-vectorshell-operations-design.md` — approved design spec
- `server/src/api/mod.rs` — source of truth for current API behavior if wording needs verification during implementation
- `server/src/client_manager/mod.rs` — source of truth for live session semantics if needed during implementation
- `dashboard/src/App.tsx` — useful current consumer example for conversation, session, tool, and artifact flows

---

### Task 1: Create the skill scaffold

**Files:**
- Create: `skill/vectorshell-operations/SKILL.md`
- Create: `skill/vectorshell-operations/references/api-endpoints.md`
- Create: `skill/vectorshell-operations/references/message-workflow.md`
- Create: `skill/vectorshell-operations/references/tool-artifact-workflow.md`
- Create: `skill/vectorshell-operations/references/output-format.md`
- Read: `docs/specs/2026-03-28-vectorshell-operations-design.md`

- [ ] **Step 1: Verify the approved spec content is present**

Read:
```text
docs/specs/2026-03-28-vectorshell-operations-design.md
```

Expected to confirm these points before writing files:
```text
- mixed trigger strategy
- conversation/message first
- brief notice before switching to tools
- artifact as supporting capability
- hybrid result reporting
- one main skill plus four reference files
```

- [ ] **Step 2: Create the directory layout**

Run:
```bash
mkdir -p skill/vectorshell-operations/references
```

Expected: directory exists with no error output.

- [ ] **Step 3: Write the main frontmatter and top-level shape**

Write `skill/vectorshell-operations/SKILL.md` with this initial content:

```markdown
---
name: vectorshell-operations
description: Use when operating live VectorShell sessions through the server API, including selecting sessions, sending conversation messages, switching to direct tool calls, or moving files through artifacts.
---

# VectorShell Operations

## Overview
Use this skill when the task is about operating a running VectorShell deployment rather than editing VectorShell source code.

Follow a conversation-first workflow: choose the target, use the message interface when the task benefits from remote-agent dialogue, switch to direct tool calls when execution is required, and use artifacts only as supporting transport for files.
```

- [ ] **Step 4: Commit the scaffold**

Run:
```bash
git add skill/vectorshell-operations/SKILL.md
git commit -m "feat: scaffold vectorshell operations skill"
```

Expected: a new commit containing the main skill file scaffold.

---

### Task 2: Write the main skill behavior

**Files:**
- Modify: `skill/vectorshell-operations/SKILL.md`
- Read: `skill/vectorshell/SKILL.md`
- Read: `docs/specs/2026-03-28-vectorshell-operations-design.md`

- [ ] **Step 1: Read the old skill to identify outdated semantics to avoid**

Read these sections from `skill/vectorshell/SKILL.md` and note what must not carry over:

```text
- any `connection_id`-first identity assumptions
- any wording that treats direct tool execution as the default
- any missing separation between trigger logic and detailed endpoint docs
```

Expected: a short scratch note or mental checklist before editing the new file.

- [ ] **Step 2: Expand `SKILL.md` with trigger boundaries and workflow**

Replace the initial body with content shaped like this:

```markdown
## When to Use
Use this skill for VectorShell operational tasks such as:
- selecting a live session
- sending conversation messages to a remote agent
- monitoring conversation or session events
- executing direct tools on a selected session
- uploading or downloading artifacts as part of a remote action
- troubleshooting API-path failures during live operations

Do not use this skill for ordinary repository coding tasks, frontend work, backend refactors, or source-only code explanations.

## Required Context
Collect the minimum execution context first:
- `server_base_url`
- `api_token`
- target selector (`install_id`, hostname, username, or explicit session choice)
- task intent

If multiple sessions match and no target is clear, ask one concise clarification question.

## Standard Workflow
1. Discover sessions and choose a target.
2. Prefer the conversation/message interface when the task benefits from dialogue, inspection, or remote reasoning.
3. Briefly tell the user before switching from message-driven work to direct tool execution.
4. Use artifacts only when files need to move through the server.
5. Report the outcome first, then the execution summary.
```

- [ ] **Step 3: Add transition, safety, and reference-loading guidance**

Append sections like this to `skill/vectorshell-operations/SKILL.md`:

```markdown
## Switching from Message to Tool Execution
Switch to direct tool execution when the task becomes a concrete remote action such as command execution, file operations, or deterministic artifact-driven work.

Before switching, say one short line such as:
- "This is more reliable through the tool API, so I'll switch to direct execution now."
- "I'll keep the same target session and move to the tool interface for this step."

## Artifacts
Treat artifacts as supporting transport, not the main control plane. Use them to stage input files for tools or retrieve output files produced by remote actions.

## Output Requirements
Always report:
1. the user-relevant result
2. selected target session
3. conversation used or created
4. tools called
5. artifact usage
6. key output or error
7. next safe step

## References
Read `references/api-endpoints.md` when you need the endpoint map.
Read `references/message-workflow.md` for conversation-first decisions.
Read `references/tool-artifact-workflow.md` for direct execution and file movement.
Read `references/output-format.md` before presenting the final result.
```

- [ ] **Step 4: Verify the file stays compact and coherent**

Run:
```bash
wc -l skill/vectorshell-operations/SKILL.md
```

Expected: concise main skill file, ideally well under 200 lines.

- [ ] **Step 5: Commit the main skill behavior**

Run:
```bash
git add skill/vectorshell-operations/SKILL.md
git commit -m "feat: define vectorshell operations workflow"
```

Expected: a commit containing the full main-skill behavior.

---

### Task 3: Write the API endpoints reference

**Files:**
- Create: `skill/vectorshell-operations/references/api-endpoints.md`
- Read: `skill/vectorshell/references/api-endpoints.md`
- Read: `server/src/api/mod.rs`
- Read: `dashboard/src/App.tsx`

- [ ] **Step 1: Gather current endpoint semantics from code and old docs**

Read for verification:
```text
skill/vectorshell/references/api-endpoints.md
server/src/api/mod.rs
dashboard/src/App.tsx
```

Verify or adjust these endpoint groups before writing:
```text
GET  /api/sessions
POST /api/conversations
POST /api/conversations/{conversation_id}/messages
GET  /api/conversations/{conversation_id}/events
POST /api/sessions/{install_id}/tools   or current path actually implemented
POST /api/artifacts
GET  /api/artifacts/{artifact_id}/download
```

Expected: confirm actual route shapes and whether REST paths use `install_id` while live payloads expose `session_id`.

- [ ] **Step 2: Write `api-endpoints.md` with current identity wording**

Create the file with this structure:

```markdown
# VectorShell API Endpoints

## Identity model
Use `install_id` as the stable external session selector when calling session-scoped API routes.
Treat `session_id` as the live runtime identifier returned by the server for active routing and event context.
Avoid older `connection_id` terminology unless the current API still returns it in a compatibility field.

## Session discovery
- `GET /api/sessions`
- Purpose: list selectable live sessions
- Expect: install identity plus live session details

## Conversation flow
- `POST /api/conversations`
- `POST /api/conversations/{conversation_id}/messages`
- `GET /api/conversations/{conversation_id}/events`

## Tool execution
- `POST /api/sessions/{install_id}/tools`
- Purpose: direct tool call on the selected session identity

## Artifact operations
- `POST /api/artifacts`
- `GET /api/artifacts/{artifact_id}`
- `GET /api/artifacts/{artifact_id}/download`
- `DELETE /api/artifacts/{artifact_id}`

## Common errors
- `401 unauthorized`
- `404 not_found`
- `409 capability_mismatch`
- timeout-style failures
```

- [ ] **Step 3: Add one concrete example per API area using current terms**

Include examples like this, corrected to match the real code if needed:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" "${BASE_URL}/api/sessions"

curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"install_id":"'"${INSTALL_ID}"'","title":"ops-session"}' \
  "${BASE_URL}/api/conversations"

curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"message":"collect basic host info"}' \
  "${BASE_URL}/api/conversations/${CONVERSATION_ID}/messages"
```

- [ ] **Step 4: Commit the endpoint reference**

Run:
```bash
git add skill/vectorshell-operations/references/api-endpoints.md
git commit -m "feat: add vectorshell operations endpoint reference"
```

Expected: a commit with the new endpoint reference.

---

### Task 4: Write the message workflow reference

**Files:**
- Create: `skill/vectorshell-operations/references/message-workflow.md`
- Read: `skill/vectorshell/references/sse-events.md`
- Read: `dashboard/src/App.tsx`

- [ ] **Step 1: Verify conversation and SSE flow from existing consumer behavior**

Read:
```text
skill/vectorshell/references/sse-events.md
dashboard/src/App.tsx
```

Extract the current mental model:
```text
- session selection happens before conversation creation
- a conversation is created per selected install when needed
- message posting and SSE observation are separate steps
- tool.started/tool.finished and agent.message can appear during message-driven work
```

- [ ] **Step 2: Create `message-workflow.md`**

Write content like this:

```markdown
# Message Workflow

## Use this path when
Prefer the message interface when the task needs inspection, explanation, environment discovery, or collaborative remote reasoning before taking direct action.

## Flow
1. List sessions.
2. Select one target.
3. Create or reuse a conversation.
4. Send the message.
5. Watch conversation events.
6. Summarize the result.

## Important event types
- `agent.message`
- `tool.started`
- `tool.finished`
- `error`

## Decision rule
Stay in message mode when the remote agent is still gathering or reasoning.
Switch to direct tool execution when the next step is a concrete command or file action.
```

- [ ] **Step 3: Add a transition example**

Include an example section like this:

```markdown
## Example
User asks to inspect a directory and decide whether a file should be downloaded.

Good message-first flow:
1. create the conversation
2. ask the remote agent to inspect the directory and identify relevant files
3. read the returned summary
4. if a specific file now needs retrieval, tell the user you are switching to the tool API and continue there
```

- [ ] **Step 4: Commit the message workflow reference**

Run:
```bash
git add skill/vectorshell-operations/references/message-workflow.md
git commit -m "feat: add vectorshell message workflow reference"
```

Expected: a commit containing the message workflow document.

---

### Task 5: Write the tool and artifact workflow reference

**Files:**
- Create: `skill/vectorshell-operations/references/tool-artifact-workflow.md`
- Read: `skill/vectorshell/references/api-endpoints.md`
- Read: `server/src/agent/file_tools.rs`
- Read: `server/src/agent/exec_tool.rs`

- [ ] **Step 1: Verify common tool names and artifact-related flows**

Read:
```text
skill/vectorshell/references/api-endpoints.md
server/src/agent/file_tools.rs
server/src/agent/exec_tool.rs
```

Confirm the documentation should cover at least:
```text
- exec
- read_file
- write_file
- upload_file
- download_file
- timeout_ms usage
- artifact-backed upload/download patterns
```

- [ ] **Step 2: Create `tool-artifact-workflow.md`**

Write content like this:

```markdown
# Tool and Artifact Workflow

## Use this path when
Use direct tool execution when the next step is a concrete remote action rather than a conversational request.

## Tool-first actions
- command execution
- deterministic file reads or writes
- upload or download operations
- artifact-backed remote tasks

## Standard flow
1. tell the user you are switching to the tool API
2. call the appropriate session tool endpoint
3. use `timeout_ms` when the task may run longer
4. summarize the result

## Artifact-assisted flow
1. upload the local file to server artifacts if needed
2. reference the artifact in the tool args
3. execute the remote action
4. download result artifacts when needed
```

- [ ] **Step 3: Add sanitization and fallback rules**

Append sections like this:

```markdown
## Sanitization
Do not surface full base64 payloads in user-facing output.
Truncate long string arguments and note the original length when useful.

## Fallbacks
If the selected session lacks the required capability, report that clearly and either choose a supported tool or stop with the capability mismatch.
If the tool times out, report the timeout and suggest retrying with a larger timeout or checking session freshness.
```

- [ ] **Step 4: Commit the tool and artifact reference**

Run:
```bash
git add skill/vectorshell-operations/references/tool-artifact-workflow.md
git commit -m "feat: add vectorshell tool and artifact workflow reference"
```

Expected: a commit containing the tool/artifact guidance.

---

### Task 6: Write the output format reference

**Files:**
- Create: `skill/vectorshell-operations/references/output-format.md`
- Read: `docs/specs/2026-03-28-vectorshell-operations-design.md`

- [ ] **Step 1: Re-read the approved reporting requirements**

Read the `Result reporting` section from:
```text
docs/specs/2026-03-28-vectorshell-operations-design.md
```

Expected to confirm the hybrid output order:
```text
1. outcome first
2. concise operational summary second
```

- [ ] **Step 2: Create `output-format.md` with the exact reporting template**

Write this template into the file:

```markdown
# Output Format

## Default response shape
Use this order:
1. result for the user
2. execution summary

## Execution summary fields
- target session: `<install_id>` and any helpful selector context
- conversation: `<conversation_id>` or `not used`
- tools: list of tool names or `none`
- artifacts: uploaded/downloaded artifact IDs or `none`
- outcome: short success or failure note
- next step: the safest useful next action

## Example success summary
- Target session: `inst-1234` (`host=demo`, `user=arch`)
- Conversation: `conv-4567`
- Tools: `exec`, `download_file`
- Artifacts: downloaded `artifact-7890`
- Outcome: command succeeded and the result file was retrieved
- Next step: inspect the downloaded artifact locally

## Example failure summary
- Target session: `inst-1234`
- Conversation: `conv-4567`
- Tools: `exec`
- Artifacts: none
- Outcome: tool call failed with `capability_mismatch`
- Next step: choose a session that advertises the required capability
```

- [ ] **Step 3: Add formatting rules for safe presentation**

Append:

```markdown
## Presentation rules
- keep the conclusion above the execution summary
- do not dump raw payloads or full binary-like strings
- include identifiers only when they help the user act on the result
- keep the summary concise and operational
```

- [ ] **Step 4: Commit the output-format reference**

Run:
```bash
git add skill/vectorshell-operations/references/output-format.md
git commit -m "feat: add vectorshell operations output format reference"
```

Expected: a commit containing the reporting template.

---

### Task 7: Validate the skill content against current repo semantics

**Files:**
- Modify: `skill/vectorshell-operations/SKILL.md`
- Modify: `skill/vectorshell-operations/references/api-endpoints.md`
- Modify: `skill/vectorshell-operations/references/message-workflow.md`
- Modify: `skill/vectorshell-operations/references/tool-artifact-workflow.md`
- Modify: `skill/vectorshell-operations/references/output-format.md`
- Read: `server/src/api/mod.rs`
- Read: `shared/src/protocol.rs`

- [ ] **Step 1: Reconcile docs with the live API naming**

Read:
```text
server/src/api/mod.rs
shared/src/protocol.rs
```

Check for any mismatch between the new docs and real code involving:
```text
- install_id vs session_id usage in endpoints and payloads
- current conversation creation request shape
- current tool call request shape
- current event names and fields
```

Expected: a short list of exact wording corrections to make before finalizing the skill.

- [ ] **Step 2: Apply wording corrections minimally**

Adjust the new files so statements like these are accurate:

```markdown
Use `install_id` for external target selection when that is how the API route is addressed.
Treat `session_id` as the live runtime identifier when discussing active routing and event context.
Document any remaining legacy field names explicitly if the API still exposes them.
```

- [ ] **Step 3: Run a repo search to ensure no new file accidentally reintroduced the old model as the primary one**

Run:
```bash
rg -n "connection_id|client_id" skill/vectorshell-operations
```

Expected: either no matches, or only tightly-scoped explanatory references that clearly label the terms as legacy compatibility names.

- [ ] **Step 4: Commit the semantic corrections**

Run:
```bash
git add skill/vectorshell-operations
git commit -m "fix: align vectorshell operations skill with current session model"
```

Expected: a cleanup commit aligning all new docs with the actual repo semantics.

---

### Task 8: Final verification and handoff

**Files:**
- Verify: `skill/vectorshell-operations/SKILL.md`
- Verify: `skill/vectorshell-operations/references/*.md`

- [ ] **Step 1: Verify all planned files exist**

Run:
```bash
ls -R skill/vectorshell-operations
```

Expected output contains:
```text
skill/vectorshell-operations/SKILL.md
skill/vectorshell-operations/references/api-endpoints.md
skill/vectorshell-operations/references/message-workflow.md
skill/vectorshell-operations/references/tool-artifact-workflow.md
skill/vectorshell-operations/references/output-format.md
```

- [ ] **Step 2: Verify the main skill is discoverable and concise**

Run:
```bash
sed -n '1,220p' skill/vectorshell-operations/SKILL.md
```

Expected to confirm:
```text
- frontmatter name is vectorshell-operations
- description starts with trigger conditions, not process summary
- conversation-first policy is explicit
- message-to-tool switch rule is explicit
- references point to the four supporting documents
```

- [ ] **Step 3: Verify the references collectively cover the spec**

Manual checklist:
```text
- endpoint map covered
- message workflow covered
- tool/artifact workflow covered
- hybrid output format covered
- install_id/session_id semantics covered
- safety/sanitization covered
```

Expected: all boxes satisfied with no placeholder text.

- [ ] **Step 4: Commit the verified final state**

Run:
```bash
git add skill/vectorshell-operations
git commit -m "docs: finalize vectorshell operations skill"
```

Expected: final commit for the completed skill package.

---

## Self-Review

### Spec coverage
- Mixed triggering strategy: covered in Task 2
- Conversation/message first: covered in Task 2 and Task 4
- Brief notice before switching to tools: covered in Task 2 and Task 4/5
- Artifact as supporting capability: covered in Task 2 and Task 5
- Hybrid reporting output: covered in Task 2 and Task 6
- One main skill plus four references: covered in Tasks 1, 3, 4, 5, and 6
- Alignment with current session semantics: covered in Task 7

### Placeholder scan
No `TODO`, `TBD`, or unspecified "write tests later" placeholders remain. Commands, file paths, and content skeletons are concrete.

### Type and naming consistency
The plan consistently uses `vectorshell-operations` as the skill name and treats `install_id` as the stable selector with `session_id` as live runtime context. Task 7 explicitly verifies and corrects any mismatch with real API shapes.
