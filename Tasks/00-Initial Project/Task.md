1. All paths are relative to the root of this git project
2. Read Resources/design-criteria.md and Resources/putio _ 2.8.14 _ putio _ SwaggerHub.pdf
3. Make a workplan meeting the following criteria
  1. Break tasks down to features and commit each feature. Add feature info to the changelog of each crate that was changed.
  2. Layout of core and cli crates, including github workflow for building the binary, running the tests, etc
  3. Our workplan should only cover the commands for login and list. Login should support saving to our config. the default location for config will be customizable in the build, but we default to <home dir>/.putiosync/config. Our list command should support subfolder and filters. 
  4. When designing commands, please enumerate options for interface and options for each command so it can be modified in the workplan by me. 
  5. Final workplan for these commands should be at Tasks/00-Initial Project/01-Workplan.md