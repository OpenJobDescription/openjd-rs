# Method used to port Open Job Description to Rust

The Open Job Description implementation was ported from Python to Rust using a heavily agentic process based on [Kiro CLI](https://kiro.dev/cli/) and [Kiro IDE](https://kiro.dev/). This document describes the high level stages the porting went through, as well as providing major prompts used along the way.

The libraries and tools ported include: openjd-model-for-python, openjd-sessions-for-python, openjd-cli, the Python implementation of the EXPR extension (unreleased) and the Python implementation of a data snapshots library developed for Deadline Cloud job attachments that complements Open Job Description and we are looking at contributing.

The high-level stages we've gone through for this port are:

1. Adding a conformance suite to openjd-specifications. The suite uses a minimal interface consisting of just the `openjd check` and `openjd run` CLI commands, and exercises a great deal of the spec by running pass and fail checks on a large number of job and environment template files.
2. Initial creation of `openjd-rs` and port. This was about establishing the structure of the Rust project into multiple crates and getting something basic working. It mostly focused on the conformance suite, as this was something the agent could use to make incremental progress.
3. Porting unit tests, evaluating quality, and refactoring. This was about translating all the Python unit tests into Rust with accommodation for language differences. The conformance suite doesn't capture many internal library design details that this part does, so the stage also involved significant refactoring.
4. Overall quality assessment and polish. This stage involves using a variety of prompting styles to repeatedly evaluate the project through different lenses, reviewing the resulting reports, and selectively updating the project where the findings are appropriate or reveal something that should change. This stage also involves integrating the library into other components that use Open Job Description, like creating Python bindings to replace the Python implementation, then porting the OpenJD Python CLI and the Deadline Cloud worker agent to use those bindings. Finally, this stage involves human code review of the project. Delaying significant human code review to this stage saves time, as the findings that a high level evaluation prompt finds can be addressed more rapidly.

This is a record of the major prompts used through the process of porting the Python Open Job Description implementation to Rust.

## STAGE 1: Initial creation

During this stage, we initialized the project and mostly let the agent work with the spec and Python implementation for reference, grinding against the conformance suite. This stage lasted until it reached 100% conformance suite success. The result at the
end, while passing the suite, was quite far from production-ready.

### The starting prompt

> So I've created a new ~/openjd-rs package whose purpose is to prototype a Rust implementation of openjd-model. I'd like to set up a project that follows Amazon/AWS best practices for open source libraries. I think https://github.com/aws/s2n-quic/ is a good example to follow, but I like the idea of https://github.com/conda/rattler (I've also seen it elsewhere) of a `crates` directory for holding crates.
>
> i want to make a rust openjobdescription library that mirrors the functionality of the reference python lib and cli and passes the conformance test suite. specs are in ~/openjd-specifications/wiki and test suite is here: ~/openjd-specifications/conformance-tests use the structure of the openjd python library (~/openjd-model-for-python) and the CLI (~/openjd-cli) as inspiration, but use rust idioms and patterns. you're done when the conformance suite runs and passes on the rust library.
>
> While you're working, use the directory ~/openjd-rs/docs as your location for specs and implementation plans for the subcomponents you make. Write specs that explain why you're making interface and implementation choices for various components, cross-referencing the openjd spec documents, the conformance suite, and the Python implementation.

### Expanded scope to include EXPR implementation and sessions library

> I've updated the implementations in openjd-model, openjd-cli, and openjd-sessions to include an implementation of the Expression Language. Please resume what you're doing, and take a moment when a good time for the interlude to review that source code and update any planning to account for those. You may also want an openjd-sessions crate

### Small adjustments

#### Separate openjd-expr crate

> I remembered another detail - I really think the expression language should be a separate crate like openjd-expr. Can you work to update the specs you have to include this separation, and keep working to adjust the implementation towards that? Keep also working on the whole thing, and step by step improving conformance suite compliance, matching the formal spec documents, and supporting equivalent functionality as the Python implementation

#### RangExpr instead of IntRangeExpr
> Note: Should use RangeExpr as the class name instead of IntRangeExpr. Like the type name in the expression language is range_expr not int_range_expr

#### Format string code should be in openjd-expr

> I have one significant note for the design and implementation. Can you review this note and incorporate it into the spec and implementation as part of what you are doing next?
>
> * The format string code should mostly be part of openjd-expr, not openjd-model. In openjd-model it should use/apply the format string classes and functions to implement the correct functionality.

#### Library code was in openjd-cli crate

> Let me ask you why the combination validation is in the CLI? Doesn't that belong in openjd-model?
>
>> You're right — the combination expression validation and the entire parameter space iteration logic belongs in openjd-model, not the CLI. ...
>
> Yes, do that refactoring now, and then continue as you were

#### Contextual keyword implementation details

> Can you look closer at the Python contextual keywords implementation and compare to the Rust one? A detail I remember is keeping the length of the replacement exactly the same length, so that error messages can use the information on the original string for best usability

#### Mixing internal/public interface

> Why does sessions need to access ast? In the Python implementation it doesn't need that level of access, there is public interface it uses

#### Hint on how to do syntax desugaring

> Have a look at how Python manages the difference between the simple script syntax sugar and the full one. It de-sugars it. Can you do similar? Probably keep both to preserve the primary representaiton, but let the downstream code use the desugared one?

#### Get it to improve the UFCS structure

> The functions themselves shouldn't have to know about UFCS. That's the point of "universal". UFCS is all about resolution and dispatch

...

> What's the reason to prefer shifting to the receiver? It might be more intuitive to shift the receiver to the args, so the function interface is more uniform.

#### Implementing zfill wrong

> If you look at the zfill definition in the spec, it has overloads for more than one type

#### Was getting some if/else details wrong

> For your change to not evaluate eval_ifexp branches during validation, can you compare with what the Python implementation does? I think it may be beneficial to evaluate, but to avoid actually calling the functions, or something similar to that

...

> The Python implementation, the spec documents in wiki/, and the RFC documents are all valuable resources to produce the highest quality Rust implementation.

#### After adding the FEATURE_BUNDLE_1 conformance tests

>  I've pulled in a branch that adds FEATURE_BUNDLE_1 conformance tests. This adds more tests, so we'll see a dip in our numbers for a bit, but it means we'll learn about features we didn't implement because the tests were lacking! Let's take a look at them, prioritize the best next thing, and go for it!

#### About the minimum int value

> Have a look at the parsing trick that the Python side used for that number

#### Unclear about the URI handling details

> Look closely at the Python implementation of the URI path to understand the detailed logic

#### Rust and Python shlex are different

> How should we change the conformance tests so that we can accept both Python and Rust's shlex? They make different choices, but both are valid

Resolved in [issue 313](https://github.com/OpenJobDescription/openjd-rs/issues/313): Expression Language §2.2.6 now defines `repr_sh` by the words a POSIX shell reads back from its output rather than by Python's exact bytes, and the conformance test runs each output through `sh` and checks those words.

#### To refactor the list[] implementation

> I'm thinking about this one, and what about this idea:
>
> Instead of having the ExprValue for list[T] be Vec, what if, when the value is not itself a list, we store Vec for list[int], Vec for list[float], Vec for list[string], etc? Then just list[list[T]] needs to be Vec for the dynamic inners
>
> Some details I think we can do:
>
> * list[bool] -> Vec with 1 byte per element
> * list[int] -> Vec with 8 bytes per element
> * list[float] -> Vec with 16 bytes per element (8 bytes for f64, 8 bytes for NULL vs string pointer - most floats won't have a pointer, so don't want to use a small string optimization here)
> * ...
> * The only one that stores Vec is list[list[T]] where the inner element is dynamically sized

### End of stage 1

The agent focused mostly on the conformance suite. With hints along the way, it was able to grind out
an implementation that passed 100% of the tests in the suite, which are based on 'openjd check' successes
and failures, and 'openjd run' output strings and failures. While it passed the conformance suite,
the code was not high quality. For example, there were only 30 unit tests, and it had made a ~2000-line function
to implement the function library instead of using multiple dispatch as described in the spec and implemented
in Python. The results were very promising, but the crates needed critical inspection, tests, and more.

## STAGE 2: Porting unit tests / evaluating quality / refactoring

During this stage, we worked through the crates in dependency order (expr -> model -> sessions -> cli, with the later-added
snapshots being independent). The primary drivers were prompts to port unit tests from Python for a particular crate, and prompts to
evaluate the quality of a crate compared to the Python implementation and to the specs.

This stage involves repeating both the unit test porting and quality evaluation prompts, repeating the following:

1. Use the prompt to generate a report.
2. Read the report, assess the findings. Select findings to address.
3. Use various prompt styles to address the findings.

Often unit tests couldn't be ported until quality findings or missing library features were ported, so switching
between the different high level evaluation was useful. Some code was in the wrong crate, and needed moving, for
example from the CLI crate into the model crate.

Near the end of this stage, when things were stabilizing, we also used prompts to generate Rust-specific specs
for each crate. Performing design reviews of those specs found issues in the specs to correct, nuances to clarify,
and choices that were bugs or not meeting our goals. For example the step space iterator was lazy except for the chunk
boundaries that were fully evaluated into a vector. The spec derived from the code included this information,
so spec review was able to identify it as an item to fix.

### For openjd-expr

#### Port Python unit tests for openjd-expr

> We've got two implementations of openjd-expr. One in Python in ~/openjd-model-for-python, and one in ~/openjd-rs. The recommended interface is defined in ~/openjd-specifications/wiki in the expression language spec. I want you to evaluate the recommended interface, then enumerate every group of unit tests in the Python implementation one by one. Make or continue with a file TEST_PORT_CHECKLIST.md with a checklist and keep track of progress in that file. Process every group of unit tests from Python one by one, and evaluate how to perform the same equivalent tests in the Rust implementation. Implement those tests in Rust, revise the Rust implementation to pass, using your best judgement on the interface that it should have. Continue working through every group of unit tests from Python, using the file with the checklist to track your progress, and do not stop until you've processed every one.

#### Evaluate the quality of openjd-expr

> I'm working on a Rust port of the Python openjd.expr library implementation. We've implemented a lot, and I want you to do a quality assessment of it. Some things I'm interested in: How does the library interface compare? Are they largely the same, or are there any substantive differences? Is the Rust library following Rust idioms, or is it to "Python"-like? Go through each file in the Python implementation of openjd.expr, and look at the corresponding Rust implementation code. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file, I will be eagerly waiting to read it.

#### Adjust ExprType to match the spec

The initial port focused on the conformance tests, which couldn't test many details of the ExprType.

> Where are our tests for the ExprType parsing and string representations?

...

> Let's first focus on ExprType. I'd like it to have the unresolved, union, and same number of type variable types as the wiki/ spec says an Python implements

#### For going through function library refactoring

> Alright, now it's time to implement Phase 2 of our function library plan. Keep track of items in the checklist of the spec:
>
> 1. Determine what to do, and make sure you understand it well. Mark the item as in-progress.
> 2. Implement the item, making sure the quality is high, including tests.
> 3. Update the item's status.
> 4. Go back to 1. for the next item if there is one.
>
> Don't stop until everything is done. When it's done, do a final quality check. Look through everything and evaluate its quality compared to the Python implementation, and compile a report of the results.

#### To improve the error message quality

> I suspect many of the failure assertions in our Rust code could benefit by adding multi-line assertions with both the message and the caret included. This is the style we've tried to adopt as much as possible in the Python code. Key attributes:
>
> 1. Assertion is about the multi-line string being inside the error message. One assertion about that, not multiple assertions about parts of it.
> 2. Assertion includes the full expected high quality error message, the line of the expression it's for, and the caret indicator.
> 3. The formatting in the Python / Rust code has the lines of the message one after another, lined up. In the Python code, we found a way to do this as a list, then concatenated the list for the assertion. This allows for easy visual inspection of the error quality.
>
>Can you go through all the Rust unit tests that are asserting something fails, and make a checklist in a doc we can use to track which >include the full proper multi-line assertion including all the elements, and which have only partial or no coverage?

... (response included "Gold standard" so I echoed it back)

> Can you choose a set of tests to process from the error assertion checklist, and upgrade them to the highest gold standard? I think you'll need to:
>
> 1. Determine the full multi-line error message. Likely run some code that prints the message instead of asserting about it.
> 2. If the error message is good, proceed to use it to make the proper assertion.
> 3. If the error message lacks a caret or has some other problem, figure out a better error message and write the test for that. Then figure how to fix the code to match or get close to that message.
> 4. Move on to the next error message.
>
> Keep going and fix a lot of error messages. It's impressive to get through some real milestones

#### Create specs for the openjd-expr crate

> I'd like to clean up and extend the spec files for the ~/openjd-rs/crates/openjd-expr crate. Currently they're scattered a bit in ~/openjd-rs/specs, and I'd like them all to be in ~/openjd-rs/specs/expr. They're also not organized, and not complete. The specification for the expr language is in ~/openjd-specifications/wiki, in the language specification and in rfcs 5, 6, and 7. I'd like the expr specs to cover the design of the full Rust crate architecture, all the function interfaces, including details of why they are as they are. You can also look at ~/openjd-model-for-python/src/openjd/expr to understand details from the Python library it was inspired by. Use only the deadline-openjd subagent.

### For openjd-model

#### Port Python unit tests for openjd-model

> We've got two implementations of openjd-model. One in Python in ~/openjd-model-for-python, and one in ~/openjd-rs. The recommended interface is defined in ~/openjd-specifications/wiki in the template schemas spec. I want you to evaluate the recommended interface, then enumerate every group of unit tests in the Python implementation one by one. Make or continue with a file TEST_PORT_CHECKLIST.md with a checklist and keep track of progress in that file. Process every group of unit tests from Python one by one, and evaluate how to perform the same equivalent tests in the Rust implementation. Implement those tests in Rust, revise the Rust implementation to pass, using your best judgement on the interface that it should have. Continue working through every group of unit tests from Python, using the file with the checklist to track your progress, and do not stop until you've processed every one.

#### Evaluate the quality of openjd-model

> I'm working on a Rust port of the Python openjd.model library implementation. We've implemented a lot, and I want you to do a quality assessment of it. Some things I'm interested in: How does the library interface compare? Are they largely the same, or are there any substantive differences? Is the Rust library following Rust idioms, or is it to "Python"-like? Go through each file in the Python implementation of openjd.model, and look at the corresponding Rust implementation code. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file, I will be eagerly waiting to read it.

#### Prompts for addressing findings

> Can you find all of the tests in Python openjd.model tests that check a failure, and create a checklist in model-failure-checklist-todos.md, then go through each one and ensure that the Rust code checks for that same failure, and makes an assertion on the full multi-line error output?

> Work through the model-failure-checklist-todos.md. For every todo in the checklist, read and understand the Python test, check for an equivalent Rust test, and if there isn't one add it. If necessary, create a template file and run both the Python and Rust openjd CLI commands to get the error message to assert about. Continue working through the checklist until it's complete.

> Can you organize the work into tasks that are a good size for subagents to work on?

> You are now in charge of directing subagents to work through this whole list. You can call subagents to work on a task, and then when they're done you should validate their work. You can use another subagent to do that validation, and then compile all the results

#### Want to make the parameters code better

The following was immediately after a quality evaluation prompt, with the results of that evaluation in context.

> Let's think about how we structure parameters in the model - particularly the division between job and task parameters. In the Python library, we have a ParameterValueType that I believe is for both? In Rust I think we should separate them into JobParameterType and TaskParameterType. All parameter values should use the ExprValue - that was added for the EXPR extension but we don't need to hold on to the legacy implementation details from before it.
>
> Can you think about these ideas more, maybe review the code as necessary, and write a model-parameters.md specification to specify how we will structure it in Rust?

#### Create specs for the openjd-model crate

> I'd like to clean up and extend the spec files for the ~/openjd-rs/crates/openjd-model crate. Currently they're scattered a bit in ~/openjd-rs/specs, and I'd like them all to be in ~/openjd-rs/specs/model. They're also not organized, and not complete. The specification for the template schemas is in ~/openjd-specifications/wiki. I'd like the model specs to cover the design of the full Rust crate architecture, all the function interfaces, including details of why they are as they are. You can also look at ~/openjd-model-for-python/src/openjd/model to understand details from the Python library it was inspired by. Use only the deadline-openjd subagent.

### For openjd-sessions

#### Port Python unit tests for openjd-sessions

> We've got two implementations of openjd-sessions. One in Python in ~/openjd-sessions-for-python, and one in ~/openjd-rs/crates/openjd-sessions. The spec for many parts of its behavior are defined in ~/openjd-specifications/wiki in various docs. I want you to evaluate the recommended interface, then enumerate every group of unit tests in the Python implementation one by one. Make or continue with a file TEST_SESSIONS_PORT_CHECKLIST.md with a checklist and keep track of progress in that file. Process every group of unit tests from Python one by one, and evaluate how to perform the same equivalent tests in the Rust implementation. Implement those tests in Rust, revise the Rust implementation to pass, using your best judgement on the interface that it should have. Continue working through every group of unit tests from Python, using the file with the checklist to track your progress, and do not stop until you've processed every one.

#### Evaluate the quality of openjd-sessons

> I'm working on a Rust port of the Python openjd.sessions library implementation in ~/openjd-rs/crates/openjd-sessions. We've implemented a lot, and I want you to do a quality assessment of it. Some things I'm interested in: How does the library interface compare? Are they largely the same, or are there any substantive differences? Is the Rust library following Rust idioms, or is it to "Python"-like? Go through each file in the Python implementation of openjd.sessions, and look at the corresponding Rust implementation code. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file, I will be eagerly waiting to read it.

#### Create a spec for getting sessions up to a better quality level

> That's a lot of missing functionality. Can you move that report into the crates/openjd-sessions/ directory so we can add it to git as a status checkpoint. Then create a spec in specs/ that lays out the Rust openjd-sessions architecture including its goals, high level file organization, categories of functionality, etc. For this work I think we'll also want to clone deadline-cloud-worker-agent from github to ~/deadline-cloud-worker-agent so that you can look at how it uses openjd-sessions in detail, because supporting it is a top priority.

#### Idempotent prompt to do part of the sessions-architecture plan

> We're working on the port of the Python ~/openjd-sessions-for-python to the Rust ~/openjd-rs/crates/openjd-sessions. See also ~/openjd-specifications/wiki for documents about expected behavior. We've documented our planned architecture in specs/sessions-architecture.md, with a section Implementation Priority that lists in order what we expect we need to do. Can you review the plan, and take the next uncompleted item to prepare for implementation? Check the current status of it in the code, and plan it out with reference to the specs and Python code as necessary. Then use subagents to implement your plan. Only use the deadline-openjd subagent. Perform additional quality checks, and adjust the plan if you discover that is necessary. Finally, update specs/sessions-architecture.md to reflect the new state of things.

#### Introduce the docker-based tests that Python has

> In the openjd-sessions Python code, there are docker-based tests. Can you fully evaluate them, compare how they work to the Rust openjd-sessions Rust crate, and make a markdown proposal document for how we port those tests into the Rust project?

#### Create specs for the openjd-sessions crate

> I'd like to clean up and extend the spec files for the ~/openjd-rs/crates/openjd-sessions crate. Currently they're scattered a bit in ~/openjd-rs/specs, and I'd like them all to be in ~/openjd-rs/specs/sessions. They're also not organized, and not complete. The specification for the template schemas is in ~/openjd-specifications/wiki. I'd like the sessions specs to cover the design of the full Rust crate architecture, all the function interfaces, including details of why they are as they are. You can also look at ~/openjd-sessions-for-python/src/openjd/sessions to understand details from the Python library it was inspired by. Use only the deadline-openjd subagent.

## Additional openjd-snapshots crate

> I've pulled in a design and implementation of something called "job attachments snapshots":
>
> * /home/markw/deadline-cloud/docs/design/job_attachments_snapshots.md
> * /home/markw/deadline-cloud/src/deadline/job_attachments/_snapshots
>
> This is a pre-release experiment, that I would like to port into a Rust implementation in a new crate:
>
> * /home/markw/openjd-rs/crates/openjd-snapshots
>
> Can you review the full job attachments snapshots design and implementation, and write a spec in /home/markw/openjd-rs/specs proposing this including how the Rust code will be structured to implement the snapshot datastructures and algorithms.

> Can you proceed with implementation? Use subagents (always just the deadline-openjd subagent). You can take the milestones and break them down into tasks that are well-sized for a subagent, and then get subagents to implement it. Also perform quality review, both your own and having subagents cross-validate the results. Note that a farm/queue is configured on this fleet, you can use its job attachments bucket for S3 testing. Also, note that disk space is low on this host, except for in /shared/. Use a subdirectory like /shared/snapshot-testing/ to work with larger datasets.

#### Evaluate the quality of openjd-snapshots

> I'm working on a Rust port of the Python job attachments snapshots library implementation (see spec ~/deadline-cloud/docs/design/job_attachments_snapshots.md, updated specs ~/specs/snapshots, Python implementation ~/deadline-cloud/src/deadline/job_attachments/_snapshots, and Rust implementation ~/openjd-rs/crates/openjd-snapshots). We've implemented a lot, and I want you to do a quality assessment of it. Some things I'm interested in: How does the library interface compare? Are they largely the same, or are there any substantive differences? Is the Rust library following Rust idioms, or is it to "Python"-like? Go through each file in the Python implementation of the library, and look at the corresponding Rust implementation code. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file, I will be eagerly waiting to read it.

#### Example of addressing a finding

> Can you plan out the following, referencing the specifications and Python code as necessary, and then use subagents to implement them? Only use the deadline-openjd subagent. Where appropriate, use red/green tdd, and make sure there are unit tests for functionality where it makes good sense.
>
> 1. Add parallel I/O for hash_upload and download (rayon or tokio tasks)
> 2. Add progress callback support to HASH, HASH_UPLOAD, DOWNLOAD

#### Port Python unit tests for openjd-snapshots

> We've got two implementations of openjd-snapshots. One in Python in ~/deadline-cloud/src/deadline/job_attachments/_snapshots, and one in ~/openjd-rs/crates/openjd-snapshots. The recommended interface is defined in ~/deadline-cloud/docs/design/job_attachments_snapshots.md and in the updated specs ~/specs/snapshots. I want you to evaluate the recommended interface, then enumerate every group of unit tests in the Python implementation one by one. Make or continue with a file TEST_PORT_CHECKLIST.md with a checklist and keep track of progress in that file. Process every group of unit tests from Python one by one, and evaluate how to perform the same equivalent tests in the Rust implementation. Implement those tests in Rust, revise the Rust implementation to pass, using your best judgement on the interface that it should have. Continue working through every group of unit tests from Python, using the file with the checklist to track your progress, and do not stop until you've processed every one. Make plans and give tasks to subagents as necessary, only using the deadline-openjd subagent.

#### Developing benchmarks

> Let's create an openjd-rs/docs/ directory, and then can you create a guide to benchmarking the openjd-snapshots library? The key operations to benchmark:
>
> HASH
>
> then with options to pick either filesystem or S3:
>
> HASH_UPLOAD DOWNLOAD
>
> We'll need an analysis about data set variety (small vs large files, many vs few files, etc), and discussion about how that maps to effective tests. For S3, we'll need to think of how we compare the library to https://github.com/peak/s5cmd or other fast S3 transfer systems. Getting a baseline from them to measure snapshots against will be essential.

> Can you run benchmarking on the HASH operation, comparing the Python and the Rust implementations?

## STAGE 3: Overall quality assurence

In this stage, we repeated the following prompts and reviewed the reports they produce iteratively. Over time, the number of real findings dwindled for each prompt style, and we moved on to craft new prompts that looked at the project through different lenses.

We also spent time reviewing the implementation code directly to validate that the underlying implementation matched our understanding of it based on the high level prompts and reports.

### Quality prompt for entire project, comparing with Python

> I'm working on the Rust implementation of Open Job Description, that includes creates openjd-expr, openjd-model, openjd-sessions, openjd-cli, and openjd-snapshots in ~/openjd-rs/crates. These have been adapted from Python implementations in ~/openjd-model-for-python, ~/openjd-sessions-for-python, ~-openjd-cli, and  ~/deadline-cloud/src/deadline/job_attachments/_snapshots. Specifications for the project are in ~/openjd-specifications/wiki, ~/openjd-specifications/rfcs, and ~/deadline-cloud/docs/design.
>
> The implementers feel confident about the quality of the result, but I need you to perform a careful, deep quality assessment of the implementation. Some things I'm interested in: How does the library interface compare? Are they largely the same, or are there any substantive differences? Is the Rust library following Rust idioms, or is it too "Python"-like? Go through each file in the Python implementation of the library, and look at the corresponding Rust implementation code. Go through the specifications and compare to the Rust code that implements that part of the specification. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file. Include your overall assessment and your recommendations for the top issues to improve. Only use the deadline-openjd subagent.

### Quality prompt for entire project, comparing Rust vs specs

> I'm working on the Rust implementation of Open Job Description, that includes creates openjd-expr, openjd-model, openjd-sessions, openjd-cli, and openjd-snapshots in ~/openjd-rs/crates. Specifications for the project are in ~/openjd-specifications/wiki, ~/openjd-specifications/rfcs, and ~/openjd-rs/specs.
>
> The implementers feel confident about the quality of the result, but I need you to perform a careful, deep quality assessment of the implementation. Some things I'm interested in: How well does the library interface match the specifications? Are they largely the same, or are there any substantive differences? Make note of any specified but unimplemented functionality, and any implemented but unspecified functionality. Is the Rust library following Rust idioms, or does it feel like it's using borrowed patterns from a different programming language? Go through the specifications and compare to the Rust code that implements that part of the specification. Make a checklist and enumerate the items one by one, writing your findings for each item into your report. Keep going through until you've analyzed all of the API features, then create an overall assessment about the differences and recommendations you have for changes or improvements to the Rust implementation. Finally, hand me the report as a Markdown file. Include your overall assessment and your recommendations for the top issues to improve. Only use the deadline-openjd subagent.

### Quality prompt for one of the crates

> You are responsible for evaluating the expr crate (known as <crate-name>) in the ~/openjd-rs project. Perform the following evaluation on the ~/openjd/rs/openjd-<crate-name> crate.
>
> There are three complementary artifacts to review:
> 1. The specifications. These are in ~/openjd-rs/specs/<crate-name>.
> 2. The implementation source. This is in ~/openjd/crates/openjd-<crate-name>/src.
> 3. The tests. There are some in the crate source, and more in ~/openjd/crates/openjd-<crate-name>/tests.
>
> Use the deadline-openjd subagent as appropriate to review and understand each of these artifacts. We want them to all be aligned with each other:
> 1. The specifications should accurately represent what the implementation source does, and should also explain goals of the system and how the implementation choices achieve those goals.
> 2. The specifications should also be complete - they should cover all parts of the implementation at a good level of abstraction to clearly communicate how the crate works.
> 3. The implementation should faithfully implement what the specifications say, and also provide a useful, ergonomic interface to write code against. Error messages should always be high quality, while variable names should always be unambiguous but still short and uncluttered. Naming should be consistent within the crate and across the other crates that are in Open Job Description.
> 4. This is a Rust library, and it should follow Rust best practices where it makes sense.
> 5. The implementaiton should have high performance, balanced with readability and maintainability. There shouldn't be a O(N^2) or worse algorithm when a O(N) or similar is possible.
> 6. The tests should confirm that the implementation does what the specifications say. The tests should be clear and well organized for review, so we can have high confidence in them. They should cover both happy path and edge cases.
> 7. The code should compile cleanly, no errors or warnings. You should run the tests and confirm they all pass.
> 8. Run additional experiments to find flaws in the library, for example if you suspect there is undefined behavior, a potential crash, or a logic bug, proceed to write failing tests that demonstrate the behavior for review in the report.
>
> Write all your results into a report at ~/openjd-rs/reports/<crate-name>-quality-evaluation-report.md. Include itemized details of all the parts you looked at, as well as a section containing your recommendations for improving the crate. Only use the deadline-openjd subagent, never use a different one.

## Example output of one of these prompts

This is the example start of the output from the above prompt for the model library.

> ### Executive Summary
>
> The openjd-model crate is a well-engineered Rust implementation of the Open Job Description 2023-09 template specification. The specifications are thorough and well-organized. The implementation closely follows both the spec and the Python reference implementation. The test suite is comprehensive with 1,516 tests covering happy paths, edge cases, and error message formatting.
>
> Two confirmed bugs were found through exploratory testing:
>
> 1. HashMap ordering nondeterminism in default parameter space iteration (correctness bug)
> 2. Description byte-length vs character-count validation (spec conformance bug)
>
> Additionally, a latent overflow bug exists in FlexFloat::Display that doesn't manifest in the current code path but would if the Display > impl were used directly.