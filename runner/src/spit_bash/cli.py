"""Command line interface for planning and running a .spitdag."""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import io
import math
import os
import signal
import subprocess
import sys
from enum import Enum
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

from .load import changed_pipeline_files, load_dag
from .cause import Why
from .model import Admission, Dag, DagError, Layout
from .plan import Status, accept_tools, adopt, explained, plan, writes
from .reuse.evidence import Compare
from .reuse.relabel import relabel
from .state.store import Store
from .execute.options import Options
from .execute.scheduler import execute
from .report import run_text
from .report.causes import decided
from .report.changes import change_lines, save_sources, saved_sources
from .report.explain import explain
from .report.plan_json import document
from .report.plan_text import adoption_line, listing, move_hint, nothing_run
from .report.reviews import review_json, review_lines
from .report.summary import Summary, left_out
from .leftovers import leftovers, lines as leftover_lines, remove as remove_leftovers
from . import clean, confirm, runs, tools
from .slurm import calllog, node, probe, site
from .slurm.controller import Controller, Start, folder as slurm_folder, planned, set_up_path, settle
from .slurm.profile import load as load_profile
from .schedule import over_memory
from .schema import VERSION as DAG_VERSION
from .selection import Selected, Selection, parse_entities, parse_stage, select

SPIT_SUFFIXES = (".spitin", ".spit", ".spitout")
# The units of --memory, as SPIT's `resources` line takes them.
UNITS = {"K": 1024, "M": 1024**2, "G": 1024**3, "T": 1024**4}

# The runner's folder, as `--dir` names it in its help.
DEFAULT = Layout(Path("ROOT") / ".spit-bash")

FILES_HELP = """\
what to run: a .spitdag (or - for one on stdin), or what `spit dag` takes,
which spit-bash then runs for you: a .spitin recipe, a .spit pipeline and
its .spitout (or - for one on stdin), or a .spit pipeline with --root"""


class Action(str, Enum):
    PLAN = "plan"
    EXPLAIN = "explain"
    RUN = "run"
    ADOPT = "adopt"
    ACCEPT_TOOLS = "accept-tools"
    LOG = "log"
    PRUNE = "prune"
    RESTORE = "restore"
    CLEAN = "clean"
    CHECK_SITE = "check-site"


# The actions that plan, and take the options that change a plan.
PLANS = (Action.PLAN, Action.EXPLAIN, Action.RUN)


HELP = {
    Action.PLAN: "report which jobs would run, be checked, skip or be blocked",
    Action.EXPLAIN: "explain one job's decision: its recorded and current fingerprint and checks,\n"
                    "the files that differ from the record, and the upstream jobs that will rewrite its inputs",
    Action.RUN: "run every job that is not current or blocked, and check current jobs whose checks changed",
    Action.ADOPT: "record jobs whose files already exist as current, without running them",
    Action.ACCEPT_TOOLS: "take the tools as they are now as those current jobs ran with, so a tool's upgrade\n"
                         "or a new tool reruns no job that has not changed otherwise",
    Action.LOG: "list each chosen job's logs, from every run that kept one, newest first",
    Action.PRUNE: "remove all but the newest run folders, keeping the log of each job whose current outputs a run made",
    Action.RESTORE: "put back the outputs a run replaced, with the records they had; the outputs there now are kept\n"
                    "in a new run folder, so a restore can be undone the same way",
    Action.CLEAN: "list every file and folder under FOLDER that the DAG does not explain, and with --yes delete them;\n"
                  "naming FOLDER says it holds pipeline outputs only",
    Action.CHECK_SITE: "test a Slurm cluster with a tiny job before a run trusts it: that an exclusive create is\n"
                       "decided by the file server, how late a file shows on another node, that every node finds this\n"
                       "spit-bash, and that Slurm accepts the profile's requests; a failed check stops later runs",
}


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="spit-bash", description="Plan and run SPIT .spitdag jobs")
    try:
        release = version("spit-bash")
    except PackageNotFoundError:
        release = "(uninstalled)"
    parser.add_argument("-V", "--version", action="version", version=f"%(prog)s {release}",
                        help="print the runner version and exit")
    parser.add_argument("--dag-version", action="version", version=str(DAG_VERSION),
                        help="print the .spitdag format version read by the runner and exit")
    subcommands = parser.add_subparsers(required=True)
    for action in Action:
        command = subcommands.add_parser(action.value, help=HELP[action], description=HELP[action],
                                         formatter_class=argparse.RawTextHelpFormatter)
        command.set_defaults(action=action)
        if action is Action.EXPLAIN:
            command.add_argument("job", type=int, metavar="ID", help="the job to explain, by its id")
        if action is Action.CHECK_SITE:
            command.add_argument("files", nargs="*", metavar="FILE",
                                 help="the DAG or recipe you will run, which says where the runner's folder is;\n"
                                      "or name the folder with --dir")
        else:
            command.add_argument("files", nargs="+", metavar="FILE", help=FILES_HELP)
        if action is Action.CLEAN:
            command.add_argument("folder", metavar="FOLDER",
                                 help="a folder under the dataset or output root that holds only what the pipeline writes")
            command.add_argument("--yes", action="store_true",
                                 help="delete what the listing names, and forget the records of the outputs deleted")
        command.add_argument("--root", help="dataset root: required when the DAG has no root,\n"
                                            "and the folder `spit dag` scans for a .spit alone")
        command.add_argument("--spit", default=os.environ.get("SPIT", "spit"),
                             help="the spit program, for a recipe or pipeline (default: $SPIT, else spit)")
        command.add_argument("--partial", action="store_true",
                             help="do what can be done: pass --partial to `spit dag`, which leaves out jobs it cannot\n"
                                  "complete, and with `run`, run the jobs that are not blocked (default: run nothing\n"
                                  "while any job is blocked)")
        choose = command.add_argument_group("choosing jobs", "Keep the jobs whose outputs match, and every job upstream of them.\n"
                                            "Repeat an option to match any of its values; different options must all match.")
        choose.add_argument("--entity", action="append", default=[], metavar="DIM=VALUE[,...]",
                            help="outputs with any listed value per dimension, such as sub=01,02,ses=baseline")
        choose.add_argument("--product", action="append", default=[], metavar="NAME", help="outputs of this product")
        choose.add_argument("--stage", action="append", default=[], metavar="NAME[/NAME...]",
                            help="jobs in this stage, or a stage inside it, such as preprocess/combine")
        if action in (Action.PLAN, Action.RUN):
            choose.add_argument("--no-upstream", action="store_true",
                                help="keep only matching jobs and read inputs from excluded jobs as they are on disk")
        command.add_argument("--dir", metavar="DIR",
                             help=f"the runner's folder, which holds its state and job logs (default: {DEFAULT.folder})")
        if action in PLANS:
            command.add_argument("--force", action="store_true",
                                 help="rerun selected command jobs even if current")
        if action in PLANS:
            command.add_argument("--compare", choices=[compare.value for compare in Compare], default="content",
                                 help="how a file counts as unchanged since its record: stat, by size and modification\n"
                                      "time alone, with no stop at an unchanged output and no reuse after a rename;\n"
                                      "content, by size and time, then by content where those differ (default);\n"
                                      "full, by content always, trusting no size or time")
        if action in PLANS:
            command.add_argument("--move-files", action="store_true",
                                 help="move outputs the state records at other paths to the paths the pipeline now gives,\n"
                                      "instead of rerunning their jobs, which `plan` lists as `place`. A tool that writes\n"
                                      "its own paths into its output is not rerun, so use this only if yours do not")
        if action is Action.CHECK_SITE:
            command.add_argument("--profile", metavar="FILE",
                                 help="the Slurm profile to check, as `run --executor slurm` takes it")
            command.add_argument("--dry", action="store_true",
                                 help="say what the check would submit and look at, and do none of it")
            command.add_argument("--timeout", type=float, default=300.0, metavar="SECONDS",
                                 help="how long to wait for the probe's tasks (default: 300)")
            command.add_argument("--enforcement", action="store_true",
                                 help="also end a job on purpose for its memory, and one for its time limit, to see that\n"
                                      "Slurm calls them OUT_OF_MEMORY and TIMEOUT, as retries expect; takes over a minute")
        if action in PLANS:
            command.add_argument("--executor", choices=["local", "slurm"], default="local",
                                 help="what runs the jobs: local, on this machine (default); slurm, as Slurm jobs\n"
                                      "this run submits and watches")
            command.add_argument("--profile", metavar="FILE",
                                 help="the Slurm profile, a TOML file of the cluster's account, partition, times,\n"
                                      "setup lines and limits (default: Slurm's own defaults)")
        if action is Action.RUN:
            command.add_argument("--bypass-verdicts", action="store_true",
                                 help="open review and screen gates for this run; artifact checks still run")
        if action in (Action.PLAN, Action.RUN):
            command.add_argument("--verbose", action="store_true",
                                 help="list every job, current ones too, and with `run` each job that did not run\n"
                                      "(default: counts, and a line for each cause that blocks jobs)")
        if action is Action.PLAN:
            command.add_argument("--json", action="store_true", help="print the plan as JSON")
        if action is Action.RUN:
            command.add_argument("-j", "--cores", type=int, default=4, metavar="CORES",
                                 help="cores to use: run jobs at once while the cores\n"
                                      "they declare fit (default: %(default)s)")
            command.add_argument("--memory", type=_memory, default=_machine_memory(), metavar="SIZE",
                                 help="memory to use, as 512M or 32G: run jobs at once while\n"
                                      "the memory they declare fits (default: the machine's)")
            command.add_argument("--no-enforce-memory", dest="enforce_memory", action="store_false",
                                 help="let a job hold more memory than it declares, and run a job\n"
                                      "declaring more than --memory alone, rather than refusing it")
            command.add_argument("--timestamp-logs", action="store_true",
                                 help="start each line of a job's log with the date, time and UTC offset at which\n"
                                      "the runner received it; a tool that buffers its output may have written it earlier")
            command.add_argument("--remove-leftovers", action="store_true",
                                 help="delete the outputs earlier runs made for jobs the pipeline no longer has,\n"
                                      "where they are as those runs left them, and forget their records")
            command.add_argument("--ask-over", type=_ask_over, default=confirm.ASK_OVER, metavar="TIME",
                                 help="at a terminal, ask before replacing the outputs of jobs whose last run took at\n"
                                      "least this long, as 90s, 2m or 1h (default: 2m)")
            command.add_argument("--yes", action="store_true",
                                 help="replace outputs without asking, as a run without a terminal does")
            command.add_argument("--keep-failed", action="store_true",
                                 help="keep what a failed job's command wrote at its outputs, to debug it")
            command.add_argument("--no-keep-replaced", dest="keep_replaced", action="store_false",
                                 help="delete the outputs a job replaces, rather than keeping them in the run's folder\n"
                                      "for `spit-bash restore`; for pipelines whose outputs are too big to keep twice")
        if action is Action.PRUNE:
            command.add_argument("--keep-runs", type=int, required=True, metavar="N",
                                 help="how many of the newest run folders to keep whole")
        if action is Action.RESTORE:
            command.add_argument("--run", required=True, metavar="NAME",
                                 help="the run whose replaced outputs to put back, by its folder's name, as `log` shows it")
    job = subcommands.add_parser("job", help="run one task of a Slurm batch, as a Slurm job does; not for people to run",
                                 description="Run task INDEX of the batch file BATCH, which a run with --executor slurm "
                                             "wrote, and write its result file.")
    job.set_defaults(action=None, entry="job")
    job.add_argument("batch", metavar="BATCH")
    job.add_argument("index", metavar="INDEX")
    check = subcommands.add_parser("probe", help="run one task of the site check, as a Slurm job does; not for people to run",
                                   description="Run task INDEX of the site check in FOLDER, which `spit-bash check-site` "
                                               "made, and write what this node found there.")
    check.set_defaults(action=None, entry="probe")
    check.add_argument("folder", metavar="FOLDER")
    check.add_argument("index", type=int, metavar="INDEX")
    check.add_argument("--timeout", type=float, default=300.0, metavar="SECONDS")
    check.add_argument("--mode", type=probe.Mode, choices=list(probe.Mode), default=probe.Mode.CHECK)
    return parser


def _memory(text: str) -> int:
    """A size as `--memory` takes it: a whole number, and a unit K, M, G or T, each 1024 times the one before."""
    unit = text[-1:].upper()
    number, scale = (text[:-1], UNITS[unit]) if unit in UNITS else (text, 1)
    if not (number.isascii() and number.isdigit()) or int(number) < 1:
        raise argparse.ArgumentTypeError(f"expected a size such as 512M or 32G, not {text!r}")
    return int(number) * scale


def _ask_over(text: str) -> float:
    try:
        return confirm.parse(text)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(str(exc)) from exc


def _machine_memory() -> int | None:
    """The machine's physical memory in bytes, or None where the system does not say."""
    try:
        memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    except (AttributeError, ValueError, OSError):
        return None
    return memory if memory > 0 else None


def _load(args: argparse.Namespace) -> Dag:
    # Cleaning uses the potential graph, including outputs a verdict withheld.
    bypass = args.action is Action.CLEAN or getattr(args, "bypass_verdicts", False)
    if not args.files[0].endswith(SPIT_SUFFIXES):
        if len(args.files) > 1:
            raise DagError(f"expected one .spitdag, got {len(args.files)} files")
        if args.files[0] == "-":
            dag = load_dag(sys.stdin, args.root, bypass_verdicts=bypass)
        else:
            with open(args.files[0], encoding="utf-8") as source:
                dag = load_dag(source, args.root, bypass_verdicts=bypass)
        # A saved DAG is a snapshot of the pipeline; one made from a recipe just now cannot be stale.
        changed = changed_pipeline_files(dag)
        if changed:
            print(f"warning: the pipeline changed since this DAG was written: {', '.join(changed)}. "
                  "Run the recipe, or spit dag again.", file=sys.stderr)
        return dag

    # `spit dag` finds a recipe's root itself; --root names the folder to scan
    # only for a pipeline given alone.
    scan = args.root is not None and args.files[0].endswith(".spit") and len(args.files) == 1
    command = [args.spit, "dag", *args.files, "--json"]
    if scan:
        command += ["--root", args.root]
    if args.partial:
        command.append("--partial")
    try:
        # spit's notes and errors go straight to the terminal.
        result = subprocess.run(command, stdout=subprocess.PIPE, text=True)
    except OSError as exc:
        raise DagError(f"cannot run `{args.spit}`: {exc.strerror}; put spit on PATH, or pass --spit or set SPIT") from exc
    if result.returncode != 0:
        raise DagError(f"`spit dag` exited with status {result.returncode}")
    return load_dag(io.StringIO(result.stdout), None if scan else args.root, bypass_verdicts=bypass)


def _check_site(args: argparse.Namespace, parser: argparse.ArgumentParser) -> int:
    """`check-site`: test the cluster with the profile, or with `--dry` say how; the status of the process."""
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and above 0")
    try:
        if args.dir:
            layout = Layout(Path(args.dir).expanduser().resolve())
        elif args.files:
            layout = _load(args).layout
        else:
            parser.error("name the runner's folder with --dir, or the DAG or recipe you will run")
        profile = load_profile(Path(args.profile).expanduser() if args.profile else None)
        if args.dry:
            probes = site.folder(layout) / "probe-DATE-PID"
            for line in site.plan_lines(profile, probes, args.timeout, args.enforcement):
                print(line)
            return 0
        set_up_path(profile)
        async def probe_site() -> site.Report:
            task = asyncio.current_task()
            assert task is not None, "site check runs in an asyncio task"
            loop = asyncio.get_running_loop()
            loop.add_signal_handler(signal.SIGTERM, task.cancel)
            try:
                return await site.check(profile, layout, args.timeout, enforcement=args.enforcement,
                                        say=lambda line: print(line, file=sys.stderr))
            finally:
                loop.remove_signal_handler(signal.SIGTERM)

        report = asyncio.run(probe_site())
    except (DagError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    for line in site.lines(report):
        print(line)
    return 1 if report.failed else 0


def _selection(args: argparse.Namespace) -> Selection:
    try:
        return Selection(
            entities=tuple(parse_entities(text) for text in args.entity),
            products=tuple(args.product),
            stages=tuple(parse_stage(text) for text in args.stage),
        )
    except ValueError as exc:
        raise DagError(str(exc)) from exc



def _plan_and_run(args: argparse.Namespace, dag: Dag, everything: Dag, selection: Selection, selected: Selected,
                  state: Store, layout: Layout, run: runs.Run | None) -> int:
    """Plan the chosen jobs, then print the plan, explain a job, or run them in `run`'s folder, as `args.action` says."""
    forced = selected.matched_ids if args.force else False
    compare = Compare(args.compare)
    pending_screens = dag.screens is not None and any(
        job.id in dag.screens.before or job.id in dag.screens.after for job in dag.jobs)
    if pending_screens and args.action is Action.RUN:
        if args.executor == "slurm":
            raise DagError("screen verdicts during a run require --executor local")
        if args.move_files:
            raise DagError("--move-files requires a plan with settled gate membership")
    controller: Controller | None = None
    start: Start | None = None
    slurm = args.executor == "slurm"
    call_log_scope = contextlib.ExitStack()
    try:
        if slurm or slurm_folder(layout).exists():
            profile = load_profile(Path(args.profile).expanduser() if args.profile else None)
            if slurm:
                set_up_path(profile)
                if args.action is Action.RUN:
                    for note in site.gate(profile, layout):
                        print(f"note: {note}", file=sys.stderr)
            controller = Controller(profile, layout)
            if args.action is Action.RUN:
                controller.lease()
                call_log_scope.enter_context(calllog.capture(controller.folder / "call_logs.jsonl"))
            start = controller.start(everything, state, args.action is Action.RUN)
            for line in start.lines:
                print(line, file=sys.stderr)
            if start.live and args.action is Action.RUN and not slurm:
                raise DagError(f"{start.live} Slurm task(s) of this folder are still queued or running. Wait for them, "
                               "run with --executor slurm to take them over, or cancel them with scancel.")
            settle(everything, state, profile.visibility)
        return _plan_and_run_with(args, dag, everything, selection, selected, state, layout, run, forced, compare,
                                  controller, start)
    except (DagError, OSError) as exc:
        if not slurm or run is None:
            raise
        print(f"error: {exc}", file=sys.stderr)
        runs.finish(run, "Slurm setup or reconciliation failed", {})
        return 2
    except KeyboardInterrupt:
        if not slurm or run is None:
            raise
        print("interrupted during Slurm setup or reconciliation; outstanding reservations kept", file=sys.stderr)
        runs.finish(run, "interrupted", {})
        return 128 + signal.SIGINT
    finally:
        try:
            call_log_scope.close()
        finally:
            if controller is not None:
                controller.close()


def _plan_and_run_with(args: argparse.Namespace, dag: Dag, everything: Dag, selection: Selection, selected: Selected,
                       state: Store, layout: Layout, run: runs.Run | None, forced: bool | frozenset[int],
                       compare: Compare, controller: Controller | None, start: Start | None) -> int:
    """`_plan_and_run`, once any Slurm tasks an earlier run left are dealt with."""
    slurm = controller is not None and args.executor == "slurm"
    if compare is not Compare.STAT:
        handed = relabel(dag, state.jobs, everything)
        # A plan sees the records handed over, but only a run stores them.
        if args.action is Action.RUN:
            state.apply(handed)
        else:
            state.assume(handed)
    # Under --force, no outputs are moved: the forced jobs run, and the rest are judged as they stand.
    claimed_dag = everything if getattr(args, "no_upstream", False) else None
    decisions = plan(dag, state, forced, compare, args.move_files and not args.force, claimed_dag=claimed_dag)
    movable = 0
    if not args.move_files and not args.force and any(
            decision.status is Status.RUN and decision.cause.why is Why.OUTPUT_MISSING and decision.cause.paths
            for decision in decisions):
        # Planning writes and moves nothing, so a second plan only asks what moving would save.
        moved = plan(dag, state, forced, compare, True, claimed_dag=claimed_dag)
        movable = sum(decision.status is Status.PLACE for decision in moved)
    bypass = getattr(args, "bypass_verdicts", False)
    if run is not None:
        if bypass:
            print("review and screen verdicts bypassed", file=sys.stderr)
        state.apply(writes(decisions))
        run.plan.write_text(document(dag, decisions, review_json(everything, decisions), bypass_verdicts=bypass) + "\n",
                            encoding="utf-8")
    blocked = any(decision.status is Status.BLOCKED for decision in decisions)
    if args.action is Action.EXPLAIN:
        decision, found = explained(dag, state, decisions, args.job, compare)
        print("\n".join(explain(dag, decisions, decision, found, layout.runs)))
        return 1 if decision.status is Status.BLOCKED else 0
    if args.action is Action.PLAN and args.json:
        print(document(dag, decisions, review_json(everything, decisions)))
    else:
        for line in change_lines(everything, decisions, state.jobs, saved_sources(layout.sources)):
            print(line)
        for line in listing(dag, decisions, args.verbose):
            print(line)
        if args.action is Action.PLAN:
            for line in review_lines(everything, decisions):
                print(line)
    if movable:
        print(move_hint(movable), file=sys.stderr)
    # A partial DAG leaves jobs out, whose records would look like those of jobs the pipeline lost.
    found = () if everything.left_out else leftovers(everything, state.jobs)
    removing = args.action is Action.RUN and args.remove_leftovers
    if args.action is Action.PLAN:
        for line in leftover_lines(found, args.verbose, False):
            print(line, file=sys.stderr)
        return 1 if blocked else 0
    if blocked and not args.partial:
        print(nothing_run(decisions), file=sys.stderr)
        runs.finish(run, "blocked", {})
        return 1

    wanted = planned(decisions)
    if not slurm and args.enforce_memory:
        running = [d.job for d in decisions if d.status in (Status.RUN, Status.MAYBE)]
        over = over_memory(running, args.memory)
        if over:
            for line in over:
                print(line, file=sys.stderr)
            print("lower the job's `resources` line, give more with --memory, "
                  "or pass --no-enforce-memory to run it alone", file=sys.stderr)
            runs.finish(run, "over memory", {})
            return 2

    if not args.yes and args.files[0] != "-" and sys.stdin.isatty():
        costly = confirm.costly(decisions, state.jobs, args.ask_over)
        if costly:
            for line in confirm.lines(costly):
                print(line, file=sys.stderr)
            print("Continue? [y/N] ", end="", file=sys.stderr, flush=True)
            if sys.stdin.readline().strip().lower() not in ("y", "yes"):
                print("nothing was run", file=sys.stderr)
                runs.finish(run, "declined", {})
                return 1

    for line in leftover_lines(found, args.verbose, removing):
        print(line, file=sys.stderr)
    if removing and found:
        state.apply(remove_leftovers(everything, found))

    # Saved before any job starts, so a run that fails or is stopped has still seen these inputs.
    save_sources(layout.sources, everything, saved_sources(layout.sources) or () if everything.left_out else ())
    make_executor = None
    if slurm:
        assert controller is not None and run is not None, "a Slurm run has its controller and run folder"
        # Slurm holds each job to its memory with cgroups; the controller runs only the profile's local jobs.
        options = Options(run, controller.profile.local_cores, None, compare, args.keep_failed, False,
                          args.timestamp_logs, args.keep_replaced)
        assert start is not None, "a Slurm run read the queue as it started"
        cancelled = controller.act(wanted, start)
        if cancelled:
            print(f"cancelled {cancelled} Slurm task(s) of work this run does not do", file=sys.stderr)
        make_executor = controller.executor(dag, options, state, wanted, start)
    else:
        options = Options(run, args.cores, args.memory, compare, args.keep_failed, args.enforce_memory,
                          args.timestamp_logs, args.keep_replaced)
    try:
        results = asyncio.run(execute(dag, decisions, state, options, run_text.printer(dag), decided, make_executor, force=forced))
    except asyncio.CancelledError:
        print("terminated; see the Slurm cancellation report" if slurm else "terminated; running jobs were stopped",
              file=sys.stderr)
        runs.finish(run, "terminated", {})
        return 128 + signal.SIGTERM
    except KeyboardInterrupt:
        print("interrupted; see the Slurm cancellation report" if slurm else "interrupted; running jobs were stopped",
              file=sys.stderr)
        runs.finish(run, "interrupted", {})
        return 128 + signal.SIGINT
    summary = Summary.of(decisions, results)
    run_text.report_failures(dag, summary, args.verbose)
    if summary.placed:
        print(run_text.moved_note(), file=sys.stderr)
    for note in run_text.varied_notes(results):
        print(note, file=sys.stderr)
    for line in run_text.replaced_lines(run):
        print(line, file=sys.stderr)
    print(run_text.summary_line(summary), file=sys.stderr)
    if slurm and controller is not None and run is not None:
        for line in controller.short_lines(results, state.jobs, run.name):
            print(line, file=sys.stderr)
    # Withheld work waits for a verdict, so it leaves the exit status alone.
    for line in review_lines(everything, decisions, results):
        print(line, file=sys.stderr)
    if args.verbose and everything.gated:
        for job in everything.gated:
            for path in job.outputs:
                if everything.path(path).exists():
                    print(f"  {path} remains from an earlier run ({job.status.name.lower()})", file=sys.stderr)
    if selection:
        for line in run_text.left_lines(left_out(everything, dag, decisions, results, state, compare)):
            print(line, file=sys.stderr)
    failed = summary.failed or summary.not_run
    if dag.tools:
        # What the jobs ran with, for the next plan to say what changed.
        tools.save(layout.tools, dag.tools)
    counts = run_text.counts(summary)
    for status in (Status.HELD, Status.REJECTED):
        count = sum(result.settled is status for result in results)
        count += sum(job.status.name.lower() == status.value for job in everything.gated)
        if count:
            counts[status.value] = count
            print(f"{count} job(s) {'held for review' if status is Status.HELD else 'not required'}", file=sys.stderr)
    if bypass:
        counts["bypass_verdicts"] = True
    runs.finish(run, "finished", counts)
    return 1 if failed or blocked else 0


def _clean(args: argparse.Namespace, dag: Dag, selection: Selection, layout: Layout, state: Store) -> int:
    """List, or with --yes delete, the strays under `args.folder`; 1 when one could not be deleted."""
    if selection:
        raise DagError("clean takes the whole DAG: jobs left out by --entity, --product or --stage "
                       "would make their outputs look like strays")
    folder = Path(args.folder).expanduser().resolve()
    if not folder.is_dir():
        raise DagError(f"{args.folder} is not a folder")
    # The runner's own folder, and the files the pipeline is read from, are never strays.
    kept = (layout.folder, *(Path(name).resolve() for name in args.files if name != "-"),
            *(path for _, path, _ in dag.pipeline_files))
    found = clean.strays(dag, folder, kept)
    shown = dag.outputs if folder.is_relative_to(dag.outputs) else dag.root
    for line in clean.lines(found, folder, shown, args.yes):
        print(line)
    if not args.yes:
        return 0
    failures, forgotten = clean.remove(dag, found, state.jobs)
    state.apply(forgotten)
    for failure in failures:
        print(f"could not delete {failure}", file=sys.stderr)
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    if args.action is None:
        if args.entry == "probe":
            return probe.main(Path(args.folder), args.index, args.timeout, args.mode)
        return node.main(args.batch, args.index)
    if args.action is Action.CHECK_SITE:
        return _check_site(args, parser)
    if args.action is Action.RUN and args.cores < 1:
        parser.error("--cores must be at least 1")
    state: Store | None = None
    try:
        selection = _selection(args)
        dag = _load(args)
        everything = dag
        total = len(dag.jobs)
        selected = select(dag, selection, no_upstream=getattr(args, "no_upstream", False))
        dag = selected.dag
        if selection:
            needed = f", and {selected.needed} they need" if selected.needed else ""
            print(f"chose {selected.matched} of {total} jobs{needed}", file=sys.stderr)
        layout = Layout(Path(args.dir).expanduser().resolve()) if args.dir else dag.layout
        writes_state = args.action in (Action.RUN, Action.ADOPT, Action.ACCEPT_TOOLS, Action.PRUNE, Action.RESTORE) or \
            (args.action is Action.CLEAN and args.yes)
        state = Store(layout.state, writes_state)
        if writes_state:
            state.lock()
        if dag.gated:
            held = sum(job.status is Admission.HELD for job in dag.gated)
            print(f"gates held {held} job(s); {len(dag.gated) - held} job(s) not required", file=sys.stderr)
        if dag.left_out:
            print(f"partial DAG: {len(dag.left_out)} output(s) left out", file=sys.stderr)

        if args.action is Action.ACCEPT_TOOLS:
            accepted = accept_tools(dag, state)
            if dag.tools:
                tools.save(layout.tools, dag.tools)
            print(f"recorded the tools as they are now for {accepted} job{'' if accepted == 1 else 's'}",
                  file=sys.stderr)
            return 0
        if args.action in PLANS:
            for line in tools.change_lines(tools.saved(layout.tools), dag.tools):
                print(line, file=sys.stderr)
        if args.action is Action.ADOPT:
            adoptions = adopt(dag, state)
            for adoption in adoptions:
                print(adoption_line(adoption))
            return 0
        if args.action is Action.LOG:
            jobs = [job for job in dag.jobs if not selection or job.id in selected.matched_ids]
            for line in run_text.log_lines(dag, jobs, runs.history(layout.runs), state.jobs):
                print(line)
            return 0
        if args.action is Action.PRUNE:
            if args.keep_runs < 0:
                raise DagError("--keep-runs must be at least 0")
            removed, logs = runs.prune(layout.runs, args.keep_runs, state.jobs)
            kept = f", keeping {logs} log{'' if logs == 1 else 's'} of current outputs" if logs else ""
            print(f"removed {removed} run folder{'' if removed == 1 else 's'}{kept}", file=sys.stderr)
            return 0
        if args.action is Action.RESTORE:
            source = layout.runs / args.run
            if not source.is_dir():
                raise DagError(f"no run {args.run} under {layout.runs}; `spit-bash log` names the runs")
            keys = {job.work.key for job in dag.jobs if job.id in selected.matched_ids} if selection else None
            undo = runs.start(layout.runs, runs.stamp())
            restored = runs.restore(runs.Run(source), undo, state, keys)
            for line in run_text.restore_lines(restored, undo):
                print(line, file=sys.stderr)
            if any(each.kept for each in restored):
                runs.finish(undo, "restored", {"from": args.run, "jobs": len(restored)})
            else:
                undo.folder.rmdir()
            return 0
        if args.action is Action.CLEAN:
            return _clean(args, dag, selection, layout, state)

        with contextlib.ExitStack() as stack:
            run = None
            if args.action is Action.RUN:
                run = runs.start(layout.runs, runs.stamp())
                # What the run prints from here on goes to its log as well.
                stack.enter_context(runs.Recording(run))
            return _plan_and_run(args, dag, everything, selection, selected, state, layout, run)
    except (DagError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    finally:
        if state is not None:
            state.close()


if __name__ == "__main__":
    raise SystemExit(main())
