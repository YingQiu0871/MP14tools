# Code signing (SignPath Foundation)

Releases can be signed for free through [SignPath Foundation](https://signpath.org).
The release workflow does nothing about it until the repository variable
`SIGNPATH_ORGANIZATION_ID` is set; without it the build is unsigned, as before.

One-time setup for the owner:

1. Apply at <https://signpath.org/apply>. The project home page (README) already
   carries the required "Code signing policy" section.
2. In SignPath, create the project and add two artifact configurations by pasting
   `.signpath/artifact-configurations/exe.xml` (slug `exe`) and `msi.xml` (slug `msi`).
3. Create a signing policy with the slug `release-signing` (or any slug, see below).
4. In the GitHub repository settings, add:
   - variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, and optionally
     `SIGNPATH_POLICY_SLUG` (default `release-signing`);
   - secret `SIGNPATH_API_TOKEN` (a SignPath API token of a submitter user).
5. Connect the project's trusted build system to GitHub in SignPath, then push a `v*` tag.

The workflow signs `mp14tools.exe`, builds the MSI from the signed exe, signs the MSI,
and computes the SHA256 values in the release notes from the final files.
