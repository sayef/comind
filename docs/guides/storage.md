# S3 and compatible storage

An index can live in a local directory or in any S3-compatible bucket (`--index-dir s3://bucket/prefix`).
comindly has no storage settings of its own: it uses the standard AWS chain, like the `aws` CLI and the
AWS SDKs. Configure access the way you would for any AWS tool.

## Credentials

The first of these that works is used:

1. Environment: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN`), or `AWS_PROFILE`.
2. Profiles in `~/.aws/config` and `~/.aws/credentials`: access keys, SSO, `credential_process`,
   assumed roles.
3. Web identity (OIDC in CI).
4. Container and EC2 instance roles.

Region comes from `AWS_REGION` or the profile's `region`. To use a profile without exporting
anything, set `aws_profile` in comindly's `config.toml` (or pick it in the app's Settings); `AWS_PROFILE`
in the environment still wins. comindly stores only the profile's name.

## SSO

```ini
[profile team-dev]
sso_session = team
sso_account_id = 123456789012
sso_role_name = Developer
region = eu-central-1

[sso-session team]
sso_start_url = https://team.awsapps.com/start
sso_region = eu-central-1
sso_registration_scopes = sso:account:access
```

Sign in once with `aws sso login --profile team-dev`; comindly, the app and agents then use the cached
session and renew short-lived credentials on their own until the session expires. Profiles that share
an `sso-session` share one sign-in, and several sessions can be signed in at once.

A profile that names an `sso_session` must not also set `sso_start_url` or `sso_region`: the AWS SDK
rejects that mix (the `aws` CLI tolerates it). Keep those two in the `[sso-session]` block only.
`comindly doctor` reports when credentials do not resolve or a sign-in is needed. In the app, a banner
says when the session for an S3 index has expired and **Sign in** runs `aws sso login` for the
profile (it needs the AWS CLI); the index loads as soon as the sign-in completes. With
`sso_auto_login = true` (Settings → Storage access) the app starts the sign-in on its own.

## MinIO, R2 and other S3-compatible stores

Point the profile (or `AWS_ENDPOINT_URL`) at the store and keep its keys in the credentials file:

```ini
# ~/.aws/config
[profile minio]
endpoint_url = http://localhost:9000
region = us-east-1

# ~/.aws/credentials
[minio]
aws_access_key_id = …
aws_secret_access_key = …
```

```sh
comindly link ../repo-a ../repo-b --index-dir s3://comindly/org      # with aws_profile = "minio"
```

Plain `http://` endpoints are allowed for local stores.
