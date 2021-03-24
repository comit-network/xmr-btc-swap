# Workflow documentation

## `ci.yml`

Defines the Continuous Integration workflow for merging into the `master` branch.

## Releases

The workflows in this repository automate various things around releases.
The functionality is composed in such a way that a human can easily start the workflow at various points, i.e. instead of being an all-or-nothing automation, we can step in where necessary.

### Preview release

We have a rolling tag `preview` that always points to HEAD of `master`.
The [preview-release.yml](./preview-release.yml) workflow moves this tag to latest HEAD every time a PR gets merged.
It also creates a corresponding GitHub "pre-release".

### Building release binaries and attaching changelog

Whenever a new release is created, the [build-release-binaries.yml](build-release-binaries.yml) workflow will build the `swap` and `asb` binaries in release mode and attach them to the release as artifacts.

Because this workflow is triggered on every release, it works for:

- automatically created `preview` releases
- releases created through the GitHub web interface
- releases created by merging release branches into `master`

### Making a new release

To create a new release, one has to:

- Create a new branch
- Update the version in the [swap/Cargo.toml](../../swap/Cargo.toml) manifest file
- Update the Changelog (convert `Unreleased` section to a release)
- Make a commit
- Open and merge a PR
- Create a release from the resulting merge commit

To avoid errors in this process, we can automate it.
The [draft-new-release.yml](./draft-new-release.yml) workflow allows the user specify the desired version and the workflow will then open a PR that automates the above.

The created branch will follow the naming of `release/X.Y.Z` for the given version.

Any time a PR with such a branch name is merged, the [create-release.yml](./create-release.yml) workflow kicks in and creates a new release based on the resulting merge commit.

Because these two workflows are de-coupled, a user is free to create a release branch themselves if they wish to do so.
They may also side-step both of these workflows by creating a release manually using the Github web interface.
