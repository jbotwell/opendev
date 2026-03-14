<!--
name: 'Tool Description: task_complete'
description: Signal task completion
version: 2.0.0
-->

Signal that you have completed the user's request. You MUST call this tool to properly end the task — do NOT just stop making tool calls.

## Usage notes

- Provide a clear summary of what was accomplished in the result parameter
- Only call this when the work is truly done — all requested changes made, tests passing, and no unresolved issues
- For subagents: this is the required way to end the subagent conversation and return results to the parent agent
- **Conversational responses**: When the user's message is conversational (greeting, question, casual chat) and no tools or code changes are needed, put your natural reply directly in the `result` parameter. The result IS what the user sees — write it as your actual response to them, not as a third-person summary like "Greeted the user". For example, if the user says "hello", the result should be something like "Hello! How can I help you today?" — not "Greeted the user".
