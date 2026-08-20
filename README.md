# intake — collecte discrète de données d'incident (prototype)

Outil DFIR pour récupérer discrètement des données auprès d'une victime pendant une
réponse à incident, **sans lien évident entre l'équipe de réponse et le point d'upload**.

Pour chaque affaire, l'outil crée un **bucket compatible S3** hébergeant une petite page
d'upload statique. La victime y dépose ses fichiers depuis un navigateur ; l'équipe les
récupère via l'API S3, puis le bucket est détruit. Prototypé hors-ligne avec **MinIO**
(Docker) ; le code S3 est portable vers un cloud français (Scaleway/OVH) en changeant
l'endpoint.

## Comment ça marche

- **Un seul bucket par affaire** (`intake-<id>-<rand>`) avec **isolation par préfixe** :
  - `site/*` → **public en lecture** (sert `index.html`, `sigv4.js`, `upload.js`, `config.json`).
  - `data/*` → **privé** (les fichiers déposés par la victime).
- La page d'upload est **sans aucune dépendance** (JS natif : signature AWS SigV4 via
  Web Crypto + upload multipart reprenable). Rien à builder, aucun `node_modules`.

### Les deux rôles / identifiants (important)

| Rôle | Identifiant | Sert à | Portée |
|------|-------------|--------|--------|
| **Ton équipe** | credentials **admin** (`INTAKE_ADMIN_*`, défaut `minioadmin`) | `create-case`, `pull-case`, `teardown-case` | administrateur |
| **La victime** | **clé scopée par affaire**, imprimée par `create-case` | uploader sur le site web | **écriture seule sous `data/*`** |

La clé de la victime **ne peut pas** lire, lister, ni écrire ailleurs que sous `data/`
(elle ne peut donc pas voir d'autres données ni altérer le site). La récupération se fait
avec **tes credentials admin** — un identifiant **différent** de celui de la victime.

## Prérequis
- **Docker** (fait tourner MinIO **et** le client `mc` — aucune install hôte de `mc`).
- **Rust** (stable).
- **Node ≥ 20** (uniquement pour les tests du site).

## Installation
```bash
docker compose up -d                 # démarre MinIO (console http://localhost:9001)
source config.example.env            # variables INTAKE_* + met bin/ sur le PATH (pour mc)
bash scripts/bootstrap-minio.sh      # attend MinIO + valide le wrapper mc (Docker)
cargo build --release
```
> `bin/mc` est un wrapper qui exécute l'image officielle `minio/mc` via Docker. Le
> `source config.example.env` met `bin/` sur le PATH ; toute commande utilisant `mc` en
> a besoin.

## Test complet, de bout en bout

### 1. Créer une affaire (équipe → credentials admin)
```bash
./target/release/intake create-case demo
```
Sortie (exemple) :
```
case_id: demo
site_url: http://localhost:9000/intake-demo-2c2e6987/site/index.html
bucket: intake-demo-2c2e6987
--- credentials à remettre à la victime (hors-bande) ---
access_key: 3BKWLK7G91BC6UPY1532
secret_key: y+Qs18nB5TU1ynWe4XjWuwcJ9G9NXJ8ozNDm+iQW
session_token: (aucun sur MinIO)
```
Note l'`site_url` et les deux valeurs de credential — c'est ce que tu transmets à la
victime **par un canal séparé** (hors-bande).

### 2. Uploader comme la victime (navigateur → clé scopée)
- **Quelle URL ?** l'`site_url` ci-dessus. Ouvre-la dans un navigateur.
  *(Sur `localhost`, le contexte est « sécurisé » et Web Crypto fonctionne ; en prod
  ce sera du HTTPS.)*
- **Quel IAM ?** la **clé scopée** de l'étape 1 : colle `access_key` dans « Access key »,
  `secret_key` dans « Secret key », laisse « Session token » **vide** (MinIO n'en a pas).
- Glisse un fichier dans la zone de dépôt. La barre progresse, puis « Terminé ✔ ».
  Le fichier est écrit sous `data/…` dans le bucket de l'affaire.

Astuce : tu peux pré-remplir les identifiants via le fragment d'URL —
`…/site/index.html#ak=<access_key>&sk=<secret_key>`.

### 3. Récupérer comme l'équipe (credentials admin — un autre IAM)
```bash
./target/release/intake pull-case demo --dest ./pulled
```
Ceci utilise **tes credentials admin** (pas la clé de la victime) et ne télécharge que le
préfixe `data/`. Les fichiers atterrissent sous `./pulled/data/…`. Vérifie l'intégrité
(taille, ou un `sha256sum`, comparé à ce que la victime a envoyé hors-bande).

### 4. Détruire l'affaire
```bash
./target/release/intake teardown-case demo --yes
```
Révoque la clé scopée **et** supprime le bucket (site + données).

## Tests automatisés
```bash
source config.example.env

# Rust (service) — nécessite MinIO lancé + bin/ sur le PATH :
cargo test

# Site (zéro dépendance, runner intégré de Node) :
eval "$(bash site-tests/provision.sh)"
node --test site-tests/*.test.mjs     # PAS `site-tests/` — Node 23 rejette un répertoire
bash site-tests/deprovision.sh

# Déploiement du site (create-case → GET public des fichiers → data/ privé → teardown) :
bash site-tests/deploy-check.sh
```
Ce que ça prouve : signature SigV4 correcte et multipart réel acceptés par MinIO ; la clé
scopée peut écrire sous `data/` mais **pas** lire/lister/altérer le site ; `site/` est
public, `data/` renvoie 403 en anonyme.

## Arrêt
```bash
docker compose down          # arrête MinIO (ajoute -v pour purger ./.minio-data)
```

## Structure
- `crates/core` — bibliothèque : plan de données S3 **portable** (`aws-sdk-s3`, path-style)
  + plan de contrôle IAM **spécifique MinIO** (via `mc`).
- `crates/cli` — binaire `intake` (façade CLI ; un futur site d'admin interne réutiliserait
  le même cœur).
- `site/` — page d'upload zéro-dépendance (SigV4 Web Crypto + multipart).
- `bin/mc` — wrapper Docker pour le client MinIO.
- `docs/superpowers/` — conception et plans d'implémentation.

## Vers la production (Scaleway/OVH)
Le seul morceau spécifique au provider est le **plan de contrôle IAM** (création/révocation
de la clé scopée) : ajouter un adaptateur `scaleway_iam`/`ovh_iam` et changer l'endpoint.
Tout le reste (data plane S3, site, multipart) est déjà portable. Ces providers n'offrant
pas de STS, la clé scopée est à durée de vie gérée manuellement (créée puis détruite au
teardown) — voir `docs/superpowers/`.
```
