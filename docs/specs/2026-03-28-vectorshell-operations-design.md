# VectorShell Operations Skill Design

## Goal
Create a reusable skill for other agents that operate a running VectorShell deployment through the server API. The skill should support session discovery and selection, conversation-based interaction through the message interface, direct tool execution when action is required, artifact upload/download flows, and consistent result reporting.

## Problem
The repository already contains a VectorShell skill draft, but it is based on outdated identity semantics such as `connection_id`. The project has since moved to the newer `install_id` and `session_id` model. A new skill is needed so other agents can operate VectorShell consistently using the current API and runtime model rather than re-deriving endpoint flow and execution policy each time.

## Users and Use Cases
Primary users are other agents working inside this repository or adjacent operational contexts. The skill should activate for requests such as:

- selecting an online VectorShell session and performing remote operations
- using the conversation/message interface to inspect or reason with the remote agent
- switching to direct tool execution for concrete remote actions
- uploading or downloading artifacts as part of an execution workflow
- troubleshooting API-side failures in the VectorShell operations path

The skill should not trigger for ordinary code editing, frontend work, backend refactors, or general explanation tasks that do not involve operating live VectorShell sessions through the API.

## Triggering Strategy
Use a mixed triggering strategy.

The skill should trigger broadly for VectorShell remote-operations tasks, including mentions of VectorShell sessions, remote exec, artifacts, conversations, messages, or API-based client operations.

The skill should not trigger for normal software development tasks in this repository unless the request is explicitly about operating a running VectorShell system.

## Recommended Skill Structure
Use one main skill with supporting reference files.

### Main skill
Directory: `skill/vectorshell-operations/`

Main document: `skill/vectorshell-operations/SKILL.md`

This file should contain:
- triggering conditions and boundaries
- execution policy and decision rules
- the standard workflow
- the rule that message/conversation is the default first path
- the rule that tool execution is introduced only when direct action is required
- the role of artifacts as a supporting transport mechanism
- result-reporting expectations
- safety and sanitization expectations

### Reference files
Directory: `skill/vectorshell-operations/references/`

Planned files:
- `api-endpoints.md`
- `message-workflow.md`
- `tool-artifact-workflow.md`
- `output-format.md`

These files should hold detailed endpoint and workflow material so the main skill stays compact and discoverable.

## Core Behavioral Design
### 1. Context discovery
The skill begins by identifying the minimum execution context:
- `server_base_url`
- `api_token`
- target selection hint such as install ID, hostname, or username
- task intent: conversation, execution, file transfer, or troubleshooting

If the target is ambiguous and multiple sessions exist, the skill should ask one concise clarification question.

### 2. Conversation-first policy
The default execution path is conversation/message first.

For tasks that involve inspection, reasoning, explanation, environmental checks, or collaborative exploration, the skill should:
1. discover sessions
2. choose a target
3. create or reuse a conversation
4. send a message
5. observe conversation or session events
6. summarize the returned result

This should be the preferred path whenever the task can benefit from remote-agent dialogue before committing to direct execution.

### 3. Explicit transition to tool execution
When the task becomes a concrete action, the skill should switch to direct tool execution.

Before switching, it should briefly tell the user that it is moving from message-based interaction to a tool-based operation. This notice should be short and informative, not a blocking approval step.

Typical tool-driven actions include:
- command execution
- file reads/writes
- upload/download operations
- deterministic remote steps that do not require further natural-language reasoning

### 4. Artifact as supporting capability
Artifacts are not the primary entrypoint. They support the conversation/tool workflow when files need to move through the server.

Typical flow:
1. upload local content into server artifact storage if needed
2. reference the artifact from the relevant tool call
3. execute the remote action
4. download output artifacts if the remote result is materialized as a file

### 5. Result reporting
The final response format should be hybrid:
1. lead with the user-relevant outcome
2. then provide a concise execution summary

The summary should include:
- selected target session
- conversation used or created
- tool calls made
- artifact usage
- key outcome or error
- recommended next step

## API and Identity Assumptions
This skill must align with the repository's current session identity model:
- `install_id` is the stable external identity used for persistent selection/history semantics
- `session_id` is the live routing identity used by the running server/client connection

The skill design should avoid older `connection_id`-based assumptions except where the current API still exposes legacy naming in request/response shapes. If any endpoint naming still uses old field names, the skill should document the current reality explicitly rather than mixing conceptual models.

## Safety Rules
The skill should emphasize:
- use supported APIs only
- prefer reversible actions before destructive ones
- do not expose raw large payloads such as base64 blobs in user-facing summaries
- sanitize long strings and binary-like inputs in logs and tool summaries
- avoid forcing unsupported tools on sessions that do not advertise the required capability

## File Layout
```text
skill/
  vectorshell-operations/
    SKILL.md
    references/
      api-endpoints.md
      message-workflow.md
      tool-artifact-workflow.md
      output-format.md
```

## Alternatives Considered
### Option A: one monolithic skill file
Pros:
- single entrypoint
- simple packaging

Cons:
- harder to maintain
- weaker separation between trigger logic and API details
- more likely to become too long and less reusable

### Option B: one main skill with reference files
Pros:
- keeps the main skill compact
- supports future API updates cleanly
- best fit for this repo and the desired agent workflow

Cons:
- requires clearer references between files

### Option C: split into two skills
Pros:
- narrower responsibilities

Cons:
- increases skill selection burden for agents
- weakens the integrated flow from message to tool to artifact

Chosen approach: Option B.

## Implementation Scope
The intended implementation after planning is:
- create a new `skill/vectorshell-operations/` directory
- write `SKILL.md`
- write four reference files
- update content so it reflects the new session identity model and current API behavior
- leave the existing older skill untouched unless a later task explicitly asks to replace or remove it

## Open Questions Resolved
- Triggering: mixed trigger model
- Primary mode: message/conversation first
- Tool transition behavior: brief notice before switching to direct tool execution
- Reporting style: result first, concise operational summary second
- Structure: one main skill plus reference files
