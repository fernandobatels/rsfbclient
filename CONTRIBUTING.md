
# Contributing to rsfbclient

Currently this github repository is the main place for official discussion (via issues and pull requests)

## Making and suggesting changes

### Pull request or issue?
* For small improvements or obvious bug fixes to code or documentation you can just make a pull request.
 * For larger changes/suggestions that modify the API or bigger architectural changes, open an issue for discussion and feedback. 
 * If you believe something should be changed in the crate's meta information (documentation, README.md, CONTRIBUTING.md, etc) you can create an issue for that. If appropriate, please provide links to existing resources that you feel may be useful.
* Updates to out-of-date information, and small changes to existing files can be submitted directly as a PR. 

### In all cases
* Please read the section about Github Workflow. It is important that submitted PRs pass our automated tests and are correctly formatted.
* If you are not sure about something, please check existing documentation before creating a PR or issue.  Since this crate is still fairly young, it is considerate, but not required, to also search for information that has already been discussed in old PRs and issues.
* Note that this repository holds the source for several crates. If proposed changes are very drastic, it may be appropriate to implement it in a separate crate and/or put it behind a feature gate.
* You may be asked to write or modify some tests after submitting an issue or PR. We will help with this where we can, if needed. See the section on testing for more information.

### A template for commit messages
(TODO)

## Github workflow
This repository uses Github actions. Upon submitting a pull request, it will automatically:
* Check that code formatting in the PR commit agrees with the output of `cargo fmt --all`
* Run all tests  from rust modules declared at the bottom of `src/tests/mod.rs`. These tests are run for many different combinations of platform and Firebird configurations.

You can (and should) run these tests for your configuration using the command `cargo test -- --test-threads 1` 

See the section on testing for more information.

## Testing
### Setting up a test database `test.fdb`
#### Using the docker image
  TODO
#### Manually
  * To set up a Firebird database for testing you will need to install the Firebird server. This process may vary by platform. (TODO: notes for different platforms/server versions)
  * With a version of the server installed, you may run these commands to create a database: (TODO)
 
### Notes about running the tests
#### TL;DR
Run `cargo test -- --test-threads 1` for testing. Running only `cargo test` may cause tests to fail when they should not.
#### Details
`cargo  test`, by default, runs tests in parallel when it is able.
This can cause problems in our case, due to concurrent database transactions competing for database resources, such as trying to drop a table while it is locked by another transaction.
Therefore, it is not expected that all tests will pass unless invoking `cargo test` as `cargo test -- --test-threads 1` to make the tests run non-concurrently and in a single thread. This is the command used by the github workflow for testing.

It is possible that in the future we will implement better support for parallel transactions in testing.

### Improvements of the testing experience
We currently have an issue for discussing improvements to test development: #59 

## Short term roadmap
  * Stabilize the high level API  and default high level implementation
  * Improve the pure rust client and Firebird wire protocol implementation
  * Integrate with existing database-agnostic crates (query builders, ORMs, etc)
  * Develop adapters and tooling for working with/migrating from other programming languages
 
## Supported use cases
For now, this crate is mostly geared towards migration of old applications that use Firebird.
We don't imagine anyone writing a new e-commerce site using Rust and Firebird, but if you are doing such a thing, or have other use specific cases in mind, please let us know.

## Notes for those coming from other programming languages
It's recognized here that users of the Firebird database are most likely to also be users of several other languages (C#, Pascal variants, Java, C++), and possibly be working with older versions of the database.

Your contributions and expertise about Firebird are very much welcome.
Even if you aren't yet comfortable submitting Rust code, it might be possible for you to help with documentation if you have some arcane knowledge about Firebird internals, since this information is a bit hard to find. 

Currently we have a single issue for questions and answers about the internals of clients here: #62

## Notes for users of non-english spoken languages
Due to its history Firebird finds much of its usage among users where English is not an official language. 
Although English is typically considered the lingua-franca of software development, there is a desire to translate resources around this crate into languages where Firebird finds high usage.
Portuguese is the most desired at present, since many of the current contributors are from Brasil.

Unfortunately the tooling for automated documentation generation in the rust ecosystem does not easily support localization to multiple languages.

If you have any suggestions to help, or are interested in doing manual translation, please let us know.