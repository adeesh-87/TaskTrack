# 🗺️ TaskTrack: Future Work & Feature Roadmap

This document outlines ideas, goals, and planned features for growing TaskTrack into a truly intelligent, notes-first, locally-hostable productivity assistant — one that protects privacy while enhancing focus and insight.

---

## 🎯 Immediate Development Goals

- [ ] Add a **Delete Task** button to the UI
- [ ] Implement a **timer** that tracks time spent on each task (with start/stop buttons)
- [ ] Fix Render.com persistence:
  - Ensure the SQLite database retains changes between cold starts
- [ ] Add **Accounts & Authentication**:
  - Per-user login
  - Tasks/notes scoped to the current user
  - Secure password storage using hashing
- [ ] Establish **Git/CI Structure**:
  - `local`: Raspberry Pi-based dev branch
  - `main`: Merged after local testing is complete
  - `prod`: Built automatically from `main` with Render-specific configs
  - `selfhost`: For on-premise use, with backend serving static frontend

---

## 🧠 LLM-Enhanced Features (Coming Soon)

- [ ] **Smart Summarization** of notes using an LLM
- [ ] **Auto-tagging** based on note content
- [ ] **Natural Language Queries** like:
  - "What did I write about FastAPI?"
  - "Show notes tagged 'deployment' made in the past month"
- [ ] **Task Extraction from Notes**:
  - Parse new entries and propose actionable items
- [ ] **Semantic Search**:
  - Index embeddings with FAISS or similar
  - Retrieve contextually relevant notes by meaning, not just keywords
- [ ] **Command Mode**:
  - "Remind me to review PRs every Friday"
  - "Tag this note 'backend' and add it as a task"

---

## 🛠 Architectural Improvements

- [ ] Refactor tags:
  - Replace comma-separated strings with normalized `Tag` table and many-to-many join
- [ ] Add `updated_at` timestamps to all notes/tasks
- [ ] Add pagination, filtering, and search to note list
- [ ] Normalize logs and use structured logging
- [ ] Add optional markdown preview in frontend (`react-markdown`)
- [ ] Enable PWA support for offline-first behavior

---

## 🧪 Collaboration & Contribution

- [ ] Add `README.md` instructions for selfhost, prod, and localtest branches
- [ ] Create a `CONTRIBUTING.md`
- [ ] Label `good first issues` and `help wanted`
- [ ] Accept community pull requests

---

## 🔒 Enterprise Readiness Goals

- [ ] Fully self-contained FastAPI app (no cloud APIs unless opted in)
- [ ] User-based note separation, secure internal hosting
- [ ] Docker container for deployment in internal infrastructure
- [ ] Zero telemetry or external tracking

---

This is just the beginning. TaskTrack is about building a clear-headed, extensible productivity tool that puts user agency and local-first data ownership at the center.

> Built with ❤️, FastAPI, and curiosity  
> — adeesh