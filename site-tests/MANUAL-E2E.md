# Test E2E manuel (navigateur)

Prérequis : MinIO lancé + bootstrap (Plan A), `intake` construit, `bin/` sur le PATH,
`./site/` contient `index.html`, `sigv4.js`, `upload.js`, `package.json`.

1. Créer une affaire (déploie `./site/` + `config.json`, imprime l'URL + les creds) :
   ```bash
   source config.example.env
   ./target/release/intake create-case demo-e2e
   ```
2. Ouvrir l'`site_url` imprimée dans un navigateur (sur `localhost` → contexte sécurisé OK).
3. Coller `access_key` / `secret_key` (session token vide), glisser un fichier de
   plusieurs centaines de Mo.
4. Vérifier : la barre progresse, puis « Terminé ✔ ».
5. Côté équipe :
   ```bash
   ./target/release/intake pull-case demo-e2e --dest ./pulled
   ```
   → le fichier déposé est présent et intègre (comparer la taille / un `sha256sum`).
6. Détruire :
   ```bash
   ./target/release/intake teardown-case demo-e2e --yes
   ```

Note : la signature et le multipart sont déjà couverts par les tests Node
(`node --test site-tests/`). Ce test manuel ne valide que le câblage UI (glisser-déposer,
barre de progression, chargement de `config.json`). Le déploiement du site (fichiers
servis publiquement + `config.json`) est vérifié automatiquement par
`site-tests/deploy-check.sh`.
