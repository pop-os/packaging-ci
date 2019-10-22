# Pop!\_OS Packaging CI

WIP Rust rewrite of the CI scripts used in Pop!_OS for automatically generating
Debian packages and repositories from the pop-os GitHub organization. Currently
requires a beta version of Rust for async/await support.

## Checklist

- [x] Fetching all GitHub repos and their branches
- [x] Checking locally-cloned git repos and updating them
- [x] Generating a git tar from each commit for each branch
- [x] Assigning a git tar to each pocket for each codename
- [x] Check if sources have been built
- [x] Build missing sources
- [ ] Memorize repos which lack debian directories
- [ ] Memorize commits which failed to build.
- [ ] Checking if a package has already been built
- [ ] Building packages with sbuild
- [ ] Creating apt repositories for each pocket and codename
- [ ] Setting GitHub statuses
- [ ] Launchpad integration
