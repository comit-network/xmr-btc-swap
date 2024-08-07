# GPG Keys

Maintainer GPG Public Keys go in this directory, for verifying signed binary hashes.

- Export your public key with:

```bash
# get your key id (16 hex chars)
$ gpg --list-secret-keys --keyid-format=long

# export your public key
$ gpg --armor --export your_key_id > your_github_handle.asc

# example
# gpg --armor --export DE8F6EA20A661697 > delta1.asc
```

- Copy that file to this directory.
- Create a new PR to add your key to the repo.

- See also "Generating a new GPG key": https://docs.github.com/en/authentication/managing-commit-signature-verification/generating-a-new-gpg-key
