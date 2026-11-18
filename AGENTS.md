# Averion Launcher — Agent Development Rules

## 1. Project Overview

Averion Launcher is a Minecraft Java Edition launcher intended to be open-source.

Primary goals:
- Reliable Minecraft installation and launching.
- Support for Linux and Windows.
- Modular, maintainable and testable architecture.
- Efficient resource usage.
- Secure Microsoft account authentication.
- A foundation that can support future extensions without unnecessary rewrites.

The first release focuses on a reliable Vanilla Minecraft launcher.

Do not implement future performance-client features unless explicitly requested.

## 2. Mandatory Development Rules

- Work only on the task explicitly assigned by the user.
- Do not start subsequent tasks automatically.
- Do not expand the scope without approval.
- Inspect relevant source files and documentation before making changes.
- Preserve existing working functionality.
- Do not overwrite or remove existing work without a clear reason and approval.
- Do not invent missing project requirements.
- If a requirement is ambiguous, explain the ambiguity and ask before making an architectural decision.
- Follow existing project conventions unless they conflict with an approved decision.

## 3. Architecture and Technology Decisions

- Build one pure Rust desktop application with Iced and Cargo. No Tauri, WebView, or JavaScript frontend.
- The target operating systems are Linux and Windows.
- The first release targets Vanilla Minecraft.
- Keep application logic independent from UI presentation wherever practical.
- Keep Minecraft version resolution, downloads, runtime management, authentication and process launching logically separated.
- Avoid premature abstractions and unnecessary crates.
- Do not introduce additional frameworks or major dependencies without explaining their necessity and obtaining approval.
- Do not change an approved technology choice without explicit user approval.

If a technical decision has not been finalized, document the alternatives and trade-offs instead of silently choosing one.

## 4. Code Quality

- Write readable, maintainable and idiomatic code.
- Prefer small modules with clear responsibilities.
- Avoid duplicated logic.
- Handle errors explicitly.
- Avoid unnecessary global state.
- Use meaningful names.
- Add comments when they explain non-obvious decisions, not when they merely repeat the code.
- Avoid placeholder implementations presented as completed features.
- Do not leave dead code or unused dependencies without justification.

## 5. Security

- Never hardcode credentials, passwords, API secrets or access tokens.
- Never log sensitive authentication data.
- Do not store account tokens in plaintext files or ordinary application settings.
- Use appropriate operating-system credential storage for authentication secrets.
- Validate external data and downloaded metadata.
- Verify downloaded files using the integrity information provided by trusted metadata.
- Avoid unsafe Rust unless necessary and justified.
- Never bypass Microsoft authentication, account ownership or entitlement checks.

Security-sensitive changes must include an explanation of the relevant risks.

## 6. Testing and Verification

- Add tests for new functionality where practical.
- Add regression tests when fixing bugs.
- Run relevant tests after making changes.
- Run formatting and lint checks when applicable.
- Consider both Linux and Windows behavior.
- Do not claim that a test passed unless it was actually executed successfully.
- Clearly report tests that could not be executed and explain why.
- Do not hide warnings, failures or known limitations.

## 7. Performance

- The complete launcher must remain usable on systems with 2 GB of total physical RAM. Validate this on Linux and Windows.
- Avoid unnecessary background tasks, polling and allocations.
- Prefer asynchronous operations for suitable I/O workloads.
- Do not optimize code without identifying a meaningful reason.
- Do not make performance improvement claims without measurements.
- Consider idle CPU usage, memory consumption, startup time and download efficiency.

## 8. Dependency Management

Before adding a dependency:
- Explain what it provides.
- Explain why existing functionality is insufficient.
- Consider maintenance status and security.
- Consider compatibility with supported platforms.
- Avoid adding multiple dependencies that solve the same problem.

Do not update unrelated dependencies during a scoped task.

## 9. Documentation

- Update documentation when behavior, architecture or setup instructions change.
- Keep documentation consistent with the actual implementation.
- Do not document planned features as completed features.
- Record important architectural decisions and their reasoning.
- Do not invent project requirements to fill gaps.

## 10. Git and File Safety

- Do not delete or overwrite user work without approval.
- Do not force-push, rewrite Git history or reset branches.
- Do not commit or push changes unless explicitly requested.
- Keep changes limited to the assigned task.
- Report modified, added and deleted files.

## 11. Required Task Workflow

For every assigned task:

1. Read this file and relevant project documentation.
2. Inspect the current implementation.
3. Summarize the intended changes.
4. Identify risks and dependencies.
5. Implement only the approved scope.
6. Run relevant tests and checks.
7. Review the final changes.
8. Report the results.

Every completion report must include:

- Summary of work.
- Files added, modified or deleted.
- Dependencies added, removed or updated.
- Tests and commands actually executed.
- Test results.
- Known issues and limitations.
- Remaining work.
- Suggested next task (without starting it).

## 12. Task Boundaries

The user controls project scope and development progression.

The agent must not:
- Skip project stages.
- Implement future-stage functionality prematurely.
- Make unapproved architectural decisions.
- Claim incomplete features are production-ready.
- Start another task without explicit authorization.

When blocked, stop and explain the issue instead of making assumptions.

## 13. Communication

- Be concise but technically precise.
- Explain important trade-offs honestly.
- Distinguish confirmed facts from assumptions.
- Report failures transparently.
- Ask questions when a missing decision materially affects implementation.

**The user's explicitly approved project decisions and requirements take precedence over assumptions or suggestions made by the agent.**
