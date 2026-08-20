# Manual E2E test (browser)

Prerequisites: MinIO running + bootstrap, `intake` built, `bin/` on PATH, and `./site/`
contains `index.html`, `sigv4.js`, `upload.js`, `package.json`.

1. Create a case (deploys `./site/` + `config.json`, prints the URL + creds):
   ```bash
   source config.example.env
   ./target/release/intake create-case demo-e2e
   ```
2. Open the printed `site_url` in a browser (on `localhost` -> secure context OK).
3. Paste `access_key` / `secret_key` (session token empty), drag a file of a few
   hundred MB.
4. Check: the progress bar advances, then "Done ✔".
5. Team side:
   ```bash
   ./target/release/intake pull-case demo-e2e --dest ./pulled
   ```
   -> the dropped file is present and intact (compare size / a `sha256sum`).
6. Destroy:
   ```bash
   ./target/release/intake teardown-case demo-e2e --yes
   ```

Note: signing and multipart are already covered by the Node tests
(`node --test site-tests/*.test.mjs`). This manual test only validates the UI wiring
(drag-drop, progress bar, loading `config.json`). Site deployment (files served publicly
+ `config.json`) is verified automatically by `site-tests/deploy-check.sh`.
