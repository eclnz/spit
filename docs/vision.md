# Vision

**Your analysis, written once, trusted always.**

SPIT has two parts that aim at different things. The language makes the plan right. The runner keeps the record of what has been done true. This page says what each part is for, and gives the principles a new feature is judged against. Where SPIT or spit-bash falls short of them, the gap is an issue to fix.

## The language

**A pipeline is your analysis, written once. It works on any dataset and is never silently wrong.**

### The problem

Research pipelines are usually bash scripts: command lines wrapped in for loops over subjects and sessions. They have no types, so the wrong file reaches the wrong step without complaint. They fail quietly, or halfway through. They assume one dataset's layout, so a pipeline that works on one dataset breaks on the next.

An analysis also works across levels: each session, then each subject, then each group, then everyone. In a script those levels become nested loops, and a grouping mistake produces a result that looks fine and isn't.

Real data makes all of this worse: a missing session, a file exported twice, a take named `03` beside `3`. Nothing notices.

### What it's like

You write what each step reads, what it makes, and the command it runs. You say which levels a step works across, and SPIT works out the grouping. The pipeline reads like the analysis. You can skim it and see what happens, with no loops, paths or bookkeeping in the way.

The dataset's oddities go in the recipe: where the files are, what's left out and why, and which of two copies to use. Another dataset needs only another recipe, and the pipeline stays the same.

Before anything runs, SPIT checks the pipeline against the data. Every input is found, every grouping is complete, and every discrepancy is named. What comes out is a plan listing every job and every command, which you can read before you trust it.

### What it is not

- **Not a programming language.** A pipeline that can compute anything can't be checked.
- **Not a guesser.** When the data is ambiguous, SPIT asks you to decide, in writing.
- **Not a runner.** It plans, and the plan is the whole of what it promises.

### Principles

1. **Never silently wrong.** A mistake in the pipeline or the data is reported before anything runs.
   *Test: if this goes wrong, does the user find out before running?*
2. **The shape of the data is declared.** Its levels, and how steps work across them, are written in the language, not coded around it.
   *Test: can SPIT check this, or is it hidden in a command?*
3. **Nothing is decided silently.** What's used, left out or chosen is written in the files.
   *Test: could a reader see this decision without running anything?*
4. **Terse.** Nothing can be removed without losing meaning.
   *Test: is anything here said twice?*
5. **One way to say each thing.** New features reuse existing forms where they can.
   *Test: does something already say this, or nearly?*

## The runner

**Start a run and walk away. Stop it, change anything, and start it again. Only what must run, runs. The machine is treated as its owners would want, and what you're told is always true.**

### The problem

An analysis is never run just once. Data arrives late, subjects are added, a step is fixed, a parameter changes, an outlier is excluded. Each time the question is the same: what needs to run again?

Rerunning everything wastes hours, and the cluster's time. Rerunning by hand risks results that mix old and new. Runs get interrupted, jobs die for lack of memory, and clusters lose nodes. After any of it, you need to know where things stand.

### What it's like

The runner takes the plan and carries it out, on a laptop or a cluster. It remembers what it has done, against which inputs and which commands. So after any change it runs exactly what's affected, and says why each job runs.

You choose what runs: one subject, one step, one stage. Try the pipeline on one subject, check the result, then widen the run to everyone. What already ran is kept and not repeated, so the pipeline grows a piece at a time.

You can stop it at any moment, and the next run picks up where it left off. Old state is always readable, so an upgrade never costs you a rerun.

On a shared cluster it behaves as the admins would want: heavy work on compute nodes, job arrays, infrequent batched queries, and nothing left behind in the queue.

At any time it can tell you what's done, what's running, what's blocked, and what needs your attention, such as a review to make or a failure to look at. When it can't be sure of something, it stops and says so.

### What it is not

- **Not where meaning is decided.** It reads the plan and nothing else of SPIT's.
- **Not a scheduler over the cluster's own.** It hands work to the machine's scheduler and follows its rules.
- **Not clever at the cost of being right.** A wasted minute is better than a wrong record.

### Principles

1. **Never lie about state.** What it reports as done, current or pending is true.
   *Test: could this ever report something that isn't so?*
2. **A good citizen of the machine.** It uses the machine as its admins intend.
   *Test: would an admin approve of how this runs?*
3. **Every stop can be resumed.** Whatever interrupts a run, the next run continues without losing work.
   *Test: if this is killed halfway, does the next run handle what's left?*
4. **You choose what runs.** Any part of the plan runs on its own, and widening a run reuses what's done.
   *Test: can the user run just this part, then grow from it without repeating work?*
5. **Says what needs attention.** The user can always tell what's happening and what to do next.
   *Test: when this goes wrong, does the user know what to do?*
6. **When unsure, stop and say so.** It never guesses about the work or the machine.
   *Test: what happens here when the runner can't be sure?*

## How they meet

**The plan is the contract.** The language decides what a job means. The runner decides whether it's done. Neither reaches into the other. The plan's format is in [the DAG format](spitdag.md).
