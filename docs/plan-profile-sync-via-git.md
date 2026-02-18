# Profile sync via Git (updated plan)

## Scope

- **Option A:** Dedicated git clone in store dir (e.g. `~/.config/gcloud/gcloud-switch/sync-repo/`). User only specifies the **remote repo URL** (branch can default to `main`).
- **Sync content:** Only `profiles.toml` (metadata: profile names, user_account, user_project, adc_account, adc_quota_project, sync_mode, active_profile). Never `adc/*.json` or any credentials.

## Data model: timestamp per profile

- Add an **optional** `updated_at` (or `updated_ts`) field to each profile so existing `profiles.toml` files still parse.
- Serialize as e.g. ISO8601 or Unix timestamp in the TOML. When writing, set `updated_at` to "now" for new or modified profiles.
- Format: extend [profile.rs](src/profile.rs) `Profile` with something like:
  - `#[serde(default)] pub updated_at: Option<i64>` (Unix secs) or `Option<String>` (ISO8601). Default = None for backward compatibility; when syncing we treat None as "old" so that any timestamp wins.

## Merge strategy (profile-by-profile, newer wins)

- **Pull:** Get the full `profiles.toml` from the repo (after `git fetch` / `git pull` in the sync clone). Parse remote file and local file. For each profile:
  - **Only on remote:** Insert into local (new profile).
  - **Only on local:** Keep (will be pushed later) or optionally add to "ignore" list later.
  - **On both:** Compare `updated_at`; **use the newer one**. If one side has no timestamp, treat as older (other wins).
- **Conflict:** When the **same profile** was changed on both machines (both have timestamps and we could define "both modified" e.g. both updated in the same sync window, or we simply compare: if timestamps equal or too close), **do not auto-merge**; instead **display which to keep** (local vs remote) and let user choose. So: minimal conflict UI for that profile only (show two versions, pick one).
- **Push:** Write current `profiles.toml` (with timestamps) to the sync clone, commit, push. No merge in push; pull first if needed so we don't overwrite remote without having merged.

## Flow (summary)

1. **Config:** Single setting — remote repo URL (e.g. in `sync-config.toml` or `[sync]` under store dir). Branch default `main`.
2. **Setup:** On first sync, clone remote into e.g. `base_dir/sync-repo` (only need that one file in the repo; user can have a repo with just `profiles.toml` or we commit only that file).
3. **Pull:** In sync subdir: `git fetch` + `git pull` (or merge). Read remote `profiles.toml`, parse. Load local `profiles.toml`. Merge: per profile, newer wins; new remote profiles inserted. If conflict (same profile, both changed): show "which to keep" (local vs remote), then write merged result to local store and optionally update sync clone and commit so state is consistent.
4. **Push:** Write local `profiles.toml` (with timestamps) to sync clone, commit, push. If remote has moved, pull first and run same merge (newer wins + conflict pick), then push.

So: **git in subdir of .config/gcloud, get full .toml from repo, compare on a profile-by-profile basis by timestamp, overwrite with newer; new profiles get inserted; when both changed, show which to keep.**

## Implementation notes

- **Crate:** `git2` or shell-out `git` for clone/fetch/pull/push. Auth = user's git (SSH or HTTPS credential helper).
- **Store:** Keep [Store](src/store.rs) for reading/writing `profiles.toml`. Sync layer: reads/writes the same file path for the clone's copy; merge produces a `ProfilesFile` that we save to local store; on push we write store's current content to clone and push.
- **CLI:** e.g. `gcloud-switch sync push`, `gcloud-switch sync pull`. Config path: next to profiles (e.g. `sync-config.toml` with `remote_url = "https://github.com/user/repo.git"`).
- **Conflict UI:** When "both changed" for one profile: print or TUI "Profile 'X' changed on both sides. (L)ocal / (R)emote?" and apply choice to merged result; then persist.

## Phasing

1. **Phase 1:** Add `updated_at` to profile serialization (optional). Config (remote URL only). Clone on first use. Push: write store's profiles.toml to clone, commit, push.
2. **Phase 2:** Pull: fetch + get remote file, merge by timestamp (newer wins, insert new). If same profile both changed, prompt which to keep. Write merged result to local store; if we had to resolve conflict, optionally write same to clone and commit before next push.
3. **Phase 3 (optional):** Clean/delete from remote only, ignore list, etc.
