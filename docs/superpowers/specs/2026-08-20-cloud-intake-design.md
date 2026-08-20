# Conception — Intake discret de données d'incident (prototype offline)

Date : 2026-08-20
Statut : en revue

## 1. Contexte et objectif

Dans le cadre de réponses à incident (DFIR), on veut collecter des données auprès
d'une victime **sans lien évident entre l'équipe de réponse et le point d'upload**.
Le mécanisme cible : un **bucket objet compatible S3** chez un **cloud français**
(Scaleway ou OVH), un **site statique** d'upload, récupération par l'équipe via l'API
S3, puis **destruction du bucket** en fin d'affaire.

Ce document décrit un **prototype développé et testé 100% hors-ligne** avec MinIO,
dont le code est directement portable vers Scaleway/OVH (tout parle l'API S3).

## 2. Décision d'architecture : « Option A » (cycle de vie manuel des credentials)

On a écarté les credentials temporaires auto-expirantes (STS) : **ni Scaleway ni OVH
n'émettent de credentials courtes durée pour l'API S3** (vérifié sur leurs docs — ils
ne proposent que des clés longue durée, valides jusqu'à suppression). STS est
spécifique à AWS.

Décision retenue : le site reste **strictement statique**. On remet au navigateur une
**clé d'accès scopée**, dont l'expiration est gérée **manuellement** par le service de
gestion (création puis suppression). C'est acceptable car le service doit *de toute
façon* gérer le cycle de vie des buckets ; ajouter la clé est marginal, et l'interface
côté victime reste minimale (moins de bugs, moins de surface d'attaque).

Sécurité de cette clé « valide jusqu'à suppression », ramenée à un rayon de souffle
minuscule par cinq garde-fous :

1. **Un bucket dédié par affaire** → la clé n'a de portée que sur ce bucket.
2. **Policy minimale** : écriture seule (`PutObject` + actions multipart) sur un
   préfixe ; ni `List`, ni `Get`, ni `Delete`, ni autre bucket.
3. **Versioning activé** → un écrasement malveillant ne détruit pas l'original
   (intégrité de la preuve).
4. **SHA-256 fourni hors-bande** par la victime → vérification d'intégrité à la
   récupération.
5. **Suppression clé + bucket** dès la récupération faite.

## 3. Périmètre du prototype

Dans le périmètre :
- Environnement MinIO local (Docker) tenant lieu de Scaleway/OVH.
- Service de gestion Rust (crate cœur + binaire CLI), avec un **adaptateur provider**.
- Cycle de vie d'une affaire : `create-case`, `teardown-case`, `pull-case`.
- Site statique d'upload avec **multipart reprenable** (fichiers multi-Go).
- Déploiement du site dans le bucket.

Hors périmètre (évolutions futures, cf. §11) :
- Site web interne d'administration (le cœur est conçu pour l'accueillir).
- Adaptateurs Scaleway/OVH réels (stubs prévus, non implémentés).
- Nom de domaine « vanity », CDN, hébergement de production.
- Authentification forte de la victime (code d'accès applicatif optionnel, plus tard).

## 4. Architecture d'ensemble

```
                        ┌─────────────────────────────────────┐
   Équipe DFIR          │  Service de gestion (Rust)           │
   (poste interne)      │                                      │
        │               │  cli (bin)  ─┐                       │
        │  create-case  │              ├─►  core (lib)         │
        ├──────────────►│  (futur:     │      │                │
        │  pull-case    │   web bin) ──┘      │ Provider trait  │
        │  teardown-case│                     │   ├─ S3DataPlane (aws-sdk-s3)
        │               │                     │   └─ IamControlPlane (spécifique)
        └───────────────┤                     ▼                │
                        │        ┌───────────────────────────┐ │
                        └───────►│  MinIO (Docker, S3 local) │◄┼─── upload multipart
                                 │   bucket <case-id>        │ │      (navigateur)
                                 │   + site statique         │ │          ▲
                                 └───────────────────────────┘ │          │
                                                               │   ┌──────┴──────┐
                                                               │   │  Victime    │
                                                               └───│  (navigateur)│
                                                                   └─────────────┘
```

Flux de confiance : l'équipe pilote le service (creds admin). La victime ne reçoit
qu'une **URL de site** + **3 valeurs de credential scopée**, transmises **hors-bande**.

## 5. Composants

### 5.1 MinIO (environnement local)
- Lancé via `docker compose` : un service MinIO, un endpoint (ex. `http://localhost:9000`),
  une clé admin (root) définie par variables d'environnement.
- Tient lieu de « Scaleway/OVH » : buckets, clés scopées (service accounts), presigned,
  multipart, CORS, versioning — toutes primitives disponibles.
- Divergence connue vs S3 « website endpoint » : MinIO n'a pas ce mode ; le site est
  servi en lecture publique (`index.html`) ou depuis un mini-serveur local. En prod,
  Scaleway et OVH ont l'hébergement statique natif → géré par l'adaptateur.

### 5.2 Service de gestion (Rust, workspace)

Workspace Cargo :
- `core` (lib) — logique métier, indépendante de la façade :
  - modèle `Case` (§6) ;
  - opérations `create_case`, `teardown_case`, `pull_case` ;
  - le trait `Provider`, composé de :
    - `S3DataPlane` — implémenté **une seule fois** via `aws-sdk-s3` (portable) :
      `create_bucket`, `enable_versioning`, `set_cors`, `deploy_site`,
      `list/get objects` (pull), `delete_bucket` ;
    - `IamControlPlane` — **spécifique provider** : `create_scoped_upload_key`,
      `delete_scoped_key`.
- `cli` (bin) — façade fine sur `core` (via `clap`). Commandes :
  - `create-case <id>` → bucket + versioning + CORS + clé scopée + déploiement du site
    → **imprime l'URL du site et les 3 valeurs de credential**.
  - `pull-case <id> [--dest DIR]` → télécharge tous les objets, vérifie les SHA-256.
  - `teardown-case <id>` → supprime la clé scopée **et** le bucket (avec confirmation).
- `provider_minio` (module/impl) — `IamControlPlane` via le CLI `mc`
  (`mc admin user svcacct add` + policy) ; `S3DataPlane` via `aws-sdk-s3`.
- Futur : `provider_scaleway`, `provider_ovh`, binaire `web`.

Config : fichier TOML + variables d'environnement (endpoint, région, creds admin,
chemin du bundle site, origine CORS autorisée).

Dépendances principales : `aws-sdk-s3`, `aws-config`, `aws-credential-types`, `tokio`,
`clap`, `serde`/`serde_json`/`toml`, `thiserror`/`anyhow`, `tracing`,
`tokio::process::Command` (pour `mc`), `sha2` (vérification d'intégrité).

### 5.3 Site statique d'upload — **zéro dépendance**
Contrainte : **aucune dépendance tierce côté site** (réduction du risque de supply
chain — c'est le composant exécuté par la victime). Donc **pas de Node/Vite, pas
d'AWS SDK, pas de build**. Fichiers statiques purs :
- `index.html` — formulaire (champs credential + zone glisser-déposer + barre de
  progression).
- `sigv4.js` — signature **AWS SigV4** en JS natif via **Web Crypto API**
  (`crypto.subtle` : HMAC-SHA256 + SHA-256 fournis par le navigateur). On n'implémente
  **aucune primitive cryptographique** — uniquement la canonicalisation de requête
  SigV4 (formatage de chaînes), testable contre MinIO et les **vecteurs de test
  officiels AWS SigV4**. `crypto.subtle` exige un contexte sécurisé, satisfait par
  `localhost` (proto) et HTTPS (prod).
- `upload.js` — orchestration multipart via `fetch` : `CreateMultipartUpload`
  (POST `?uploads`), `UploadPart` (PUT `?partNumber&uploadId`, en-tête
  `x-amz-content-sha256: UNSIGNED-PAYLOAD` pour éviter de re-lire chaque chunk),
  `CompleteMultipartUpload` (POST, corps XML des parts/ETags). Gère le découpage, la
  **reprise** (ré-émission des parts échouées) et une **barre de progression**.
- La victime **colle 3 valeurs** (access key / secret / session token — vide sur MinIO,
  champ prévu pour un futur STS AWS) ; alternative : **fragment d'URL** (`#...`).
- Config non-secrète (endpoint, région, bucket data) injectée par affaire via un
  `config.json` déposé à côté du site. **Adressage path-style** (`endpoint/bucket/key`)
  → site et data partagent la même origine → **pas de CORS nécessaire dans le proto**
  (CORS conservé côté service pour le mode virtual-host de prod).
- SHA-256 navigateur : faisable pour **petits fichiers** (`crypto.subtle.digest`) ;
  pour le multi-Go, non praticable (Web Crypto n'a pas de hash incrémental → tout en
  mémoire). Intégrité des gros fichiers assurée par TLS + versioning + clé écriture-
  seule ; ETag S3 comparable côté équipe.

Total : ~300-400 lignes de JS auditables, sans `node_modules`. Compromis assumé : on
possède la logique SigV4/multipart au lieu de la déléguer au SDK.

### 5.4 Récupération équipe
- `pull-case` (creds admin) → liste + télécharge les objets du bucket, calcule le
  SHA-256 de chaque objet et le **compare au hash fourni hors-bande** (fichier de
  manifeste ou saisie). Signale toute divergence.

## 6. Modèle de données

`Case` (persisté localement par le service, ex. `cases/<id>.json`) :
- `id` : identifiant d'affaire (slug).
- `bucket` : nom du bucket dédié (ex. `intake-<id>-<rand>`).
- `prefix` : préfixe d'upload (ex. `data/`).
- `provider` : `minio` (proto) | `scaleway` | `ovh`.
- `scoped_key_id` : identifiant de la clé scopée (pour la suppression).
- `site_url` : URL du site déployé.
- `created_at` / `state` : `active` | `torn_down`.
- `expected_hashes` : SHA-256 attendus, renseignés hors-bande (optionnel).

## 7. Flux détaillés

### 7.1 `create-case`
1. Génère `bucket` (nom aléatoire non devinable) et `prefix`.
2. `create_bucket` + `enable_versioning`.
3. `set_cors` : méthodes `PUT, POST, GET, HEAD` ; `ExposeHeaders: [ETag]`
   (indispensable pour finaliser un multipart) ; origine = origine du site.
4. `create_scoped_upload_key` : clé + policy **écriture seule** sur `bucket/prefix/*`
   (§8).
5. `deploy_site` : dépose les assets statiques dans le bucket (ou bucket-site dédié).
6. Persiste le `Case`, imprime **URL du site + 3 valeurs**.

### 7.2 Upload (victime)
1. Ouvre l'URL, colle les 3 valeurs (ou fragment d'URL).
2. Glisse le fichier ; `lib-storage` initie le multipart, envoie les parts, reprend
   sur coupure, affiche la progression.
3. À la fin, affiche le SHA-256 local à communiquer hors-bande.

### 7.3 `pull-case`
1. Liste et télécharge tous les objets (creds admin).
2. Vérifie les SHA-256 vs `expected_hashes`.

### 7.4 `teardown-case`
1. Demande confirmation.
2. `delete_scoped_key`.
3. `delete_bucket` (vide d'abord toutes versions, puis supprime).
4. Marque le `Case` `torn_down`.

## 8. Modèle de sécurité — policy de la clé scopée

Policy (style AWS/MinIO) attachée à la clé remise à la victime :

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:PutObject",
        "s3:AbortMultipartUpload",
        "s3:ListMultipartUploadParts"
      ],
      "Resource": "arn:aws:s3:::<bucket>/<prefix>/*"
    }
  ]
}
```

- `PutObject` couvre aussi `CreateMultipartUpload` / `UploadPart` /
  `CompleteMultipartUpload` (mapping IAM S3).
- Pas de `GetObject`, `ListBucket`, `DeleteObject`, ni autre ressource.
- Versioning activé sur le bucket → un `PutObject` écrasant crée une nouvelle version
  sans détruire l'original.
- En prod, la même policy est créée via l'IAM du provider (adaptateur) ; le format
  ARN/ressource peut différer légèrement (à ajuster dans l'impl provider).

## 9. Gestion d'erreurs
- Toute opération de `create-case` est **idempotente / réversible** : en cas d'échec en
  cours de route, `teardown-case` nettoie ce qui a été créé.
- `pull-case` : une divergence de hash **n'interrompt pas** le téléchargement mais est
  signalée en fin de run (rapport clair des objets OK / KO).
- `teardown-case` : suppression du bucket seulement après vidage complet des versions ;
  échec partiel → état laissé cohérent + message actionnable.
- Erreurs `mc` / SDK remontées avec contexte (bucket, opération), pas d'échec silencieux.

## 10. Portabilité vers Scaleway / OVH

| Élément | Portable tel quel ? |
|---|---|
| `S3DataPlane` (bucket, versioning, CORS, deploy, pull, delete) via `aws-sdk-s3` | ✅ (changer endpoint + creds) |
| `IamControlPlane` (créer/supprimer la clé scopée) | ⚠️ Adaptateur provider (IAM Scaleway / OVH) |
| Site statique + upload multipart | ✅ (changer endpoint + region) |
| Hébergement statique du site | ⚠️ MinIO (public-read) vs website endpoint natif Scaleway/OVH |

Seuls les points ⚠️ demandent une implémentation provider ; le reste est validé offline.

## 11. Évolutions futures
- **Site web interne** d'administration : nouveau binaire `web` (ex. `axum`) réutilisant
  `core` — aucune réécriture de la logique.
- Adaptateurs `scaleway` / `ovh` réels.
- Code d'accès applicatif optionnel (assurance supplémentaire côté victime).
- Domaine neutre + TLS de production, hébergement statique natif.
- Le champ « session token » du site est déjà prévu si migration future vers un
  provider à STS (ex. AWS).

## 12. Stratégie de test
- **Cœur (`core`)** : tests d'intégration contre MinIO en Docker (cycle complet
  create → upload simulé → pull → teardown), avec vérification que la clé scopée
  **ne peut pas** lire/lister/supprimer (tests négatifs de la policy).
- **Multipart** : test d'upload d'un fichier volumineux (généré) et d'une **reprise**
  après coupure simulée.
- **Intégrité** : test de vérification SHA-256 (cas OK et cas de divergence).
- **SigV4 (`sigv4.js`)** : tests unitaires contre les **vecteurs officiels AWS SigV4**,
  exécutés via le **runner intégré de Node** (`node --test`) — Node sert uniquement de
  lanceur de test, **aucun paquet npm**, rien n'est embarqué. Le même `sigv4.js`/
  `upload.js` tourne au navigateur (fetch + `crypto.subtle` communs à Node 20+ et au
  navigateur).
- **Multipart** : upload réel d'un fichier volumineux (généré) contre MinIO via une clé
  scopée, exécuté depuis `node --test`, + test de **reprise** après part échouée.
- **Site** : test manuel de bout en bout dans un navigateur contre MinIO local.

## 13. Décisions verrouillées (ex-points ouverts)
- **Persistance des `Case`** : JSON local, un fichier par affaire (`cases/<id>.json`).
- **Deux buckets par affaire** : `intake-data-<id>-<rand>` (privé, versioning, clé
  scopée écriture-seule) + `intake-site-<id>-<rand>` (public-read, sert le site). La
  séparation garantit qu'aucune policy publique ne touche jamais le bucket de données.
- **Manifeste de hashes hors-bande** : JSON `{ "<clé objet>": "<sha256 hex>" }`, fourni
  par un canal séparé, comparé par `pull-case`.
