import csv
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_cli import call


PIPELINE = """\
source raw : Text [sub]
operation copy_raw(input: Text) -> Text
operation finish(input: Text) -> Text
operation merge(first: many Text, second: many Text) -> Text
command copy_raw: cp {input} {@output}
command finish: cp {input} {@output}
command merge: printf "%s\u005c\u005cn" @join(first.sub, ",") {first.sub} {second.sub} > {@output}
made = copy_raw(raw) @review
other = finish(raw)
last = finish(made)
total = merge(made @vary(sub), other @vary(sub))
path raw: data/{sub}.txt
path made: work/{sub}.txt
path other: other/{sub}.txt
path last: last/{sub}.txt
path total: total.txt
"""


QC_PIPELINE = """\
source raw : Text [sub]
operation fit(input: Text) -> Fit
operation plot(input: Fit) -> Figure
operation compose(input: Figure) -> Evidence
operation cohort(items: many Fit) -> Table
command fit: cp {input} {@output}
command plot: cp {input} {@output}
command compose: cp {input} {@output}
command cohort: cat {items} > {@output}
fits = fit(raw) @review(overview)
plots = plot(fits)
overview = compose(plots)
stage analysis:
    total = cohort(fits @vary(sub))
path raw: data/{sub}.txt
path fits: work/{sub}.txt
path plots: plots/{sub}.txt
path overview: qc/{sub}.txt
path total: total.txt
"""


class ReviewTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.csv = self.root / "qc.csv"
        (self.root / "data").mkdir()
        for sub in ("01", "02"):
            (self.root / f"data/{sub}.txt").write_text(sub + "\n")
        (self.root / "recipe.spitin").write_text("pipeline pipeline.spit\nroot .\nreview made from qc.csv\n")

    def saved(self, pipeline=PIPELINE, *, reviewed=("made",)):
        spit = os.environ.get("SPIT")
        if not spit:
            self.skipTest("set SPIT to the compiler")
        path = self.root / "jobs.spitdag"
        (self.root / "pipeline.spit").write_text(pipeline)
        recipe = (self.root / "recipe.spitin").read_text()
        if reviewed != ("made",):
            lines = []
            for line in recipe.splitlines(keepends=True):
                if line.startswith("review made from "):
                    lines.extend(
                        line.replace("review made from ", f"review {product} from ", 1) for product in reviewed
                    )
                else:
                    lines.append(line)
            recipe = "".join(lines)
        compiled_recipe = self.root / "compiled.spitin"
        compiled_recipe.write_text(recipe)
        process = subprocess.run([spit, "dag", str(compiled_recipe), "-o", str(path)], capture_output=True, text=True)
        self.assertEqual(process.returncode, 0, process.stderr)
        return str(path)

    def run_saved(self, dag, *flags):
        code, _, err = call("run", dag, "--cores", "2", *flags)
        self.assertEqual(code, 0, err)
        return err

    def test_selected_analysis_runs_evidence_then_filters_the_cohort(self):
        dag = self.saved(QC_PIPELINE.replace("@review(overview)", "@review(plots, overview)"), reviewed=("fits",))
        report = self.run_saved(dag, "--stage", "analysis")
        self.assertIn("held for review", report)
        self.assertFalse((self.root / "total.txt").exists())
        with self.csv.open(newline="") as source:
            reader = csv.DictReader(source)
            header = reader.fieldnames
            rows = list(reader)
        self.assertEqual([row["@artifact"] for row in rows], ["fits[sub=01]", "fits[sub=02]"])
        for row, sub in zip(rows, ("01", "02")):
            paths = [Path(path) for path in row["@evidence"].splitlines()]
            self.assertEqual([path.parent.name for path in paths], ["plots", "qc"])
            self.assertEqual([path.read_text() for path in paths], [sub + "\n", sub + "\n"])
        rows[0]["verdict"] = "pass"
        rows[1]["verdict"] = "fail"
        with self.csv.open("w", newline="") as output:
            writer = csv.DictWriter(output, header)
            writer.writeheader()
            writer.writerows(rows)
        self.run_saved(dag, "--product", "total")
        self.assertEqual((self.root / "total.txt").read_text(), "01\n")

    def test_multi_output_call_records_each_products_own_verdict(self):
        pipeline = """\
source raw : Text [sub]
operation split(input: Text) -> (one: Text, two: Text)
operation finish(input: Text) -> Text
command:
    split:
        cp {input} {one}
        cp {input} {two}
    finish: cp {input} {@output}
first, second = split(raw) @review
first_done = finish(first)
second_done = finish(second)
path raw: data/{sub}.txt
default path: out/{@product}/{sub}.txt
"""
        (self.root / "recipe.spitin").write_text(
            "pipeline pipeline.spit\nroot .\nreview first from first.csv\nreview second from second.csv\n"
        )
        dag = self.saved(pipeline)
        self.run_saved(dag)
        first = self.root / "first.csv"
        second = self.root / "second.csv"
        self.assertIn("first[sub=01]", first.read_text())
        self.assertIn("second[sub=01]", second.read_text())
        first.write_text("sub,verdict\n01,pass\n")
        second.write_text("sub,verdict\n01,fail\n")
        self.run_saved(dag)
        self.assertTrue((self.root / "out/first_done/01.txt").exists())
        self.assertFalse((self.root / "out/second_done/01.txt").exists())

    def test_evidence_can_run_again_after_rejection(self):
        dag = self.saved(QC_PIPELINE, reviewed=("fits",))
        self.csv.write_text("sub,verdict\n01,fail\n02,pass\n")
        self.run_saved(dag, "--product", "overview")
        self.assertEqual((self.root / "qc/01.txt").read_text(), "01\n")
        self.assertFalse((self.root / "total.txt").exists())

    def test_another_review_still_gates_the_evidence_reader(self):
        second = self.root / "independent.csv"
        (self.root / "recipe.spitin").write_text(
            "pipeline pipeline.spit\nroot .\nreview fits from qc.csv\nreview independent from independent.csv\n"
        )
        dag = self.saved(
            QC_PIPELINE.replace("operation plot(input: Fit)", "operation plot(input: Fit, other: Fit)")
            .replace("plots = plot(fits)", "independent = fit(raw) @review\nplots = plot(fits, independent)")
            .replace("path fits:", "path independent: independent/{sub}.txt\npath fits:")
        )
        self.run_saved(dag, "--product", "total")
        self.assertTrue(second.exists())
        self.assertFalse(self.csv.exists())
        self.assertFalse((self.root / "plots/01.txt").exists())
        second.write_text("sub,verdict\n01,pass\n02,pass\n")
        self.run_saved(dag, "--product", "total")
        self.assertTrue(self.csv.exists())
        self.assertFalse((self.root / "total.txt").exists())

    def test_evidence_failure_does_not_publish_a_stale_figure(self):
        dag = self.saved(
            QC_PIPELINE.replace("command compose: cp {input} {@output}", "command compose: false {input} {@output}"),
            reviewed=("fits",),
        )
        stale = self.root / "qc/01.txt"
        stale.parent.mkdir()
        stale.write_text("stale")
        code, _, err = call("run", dag, "--product", "total")
        self.assertEqual(code, 1, err)
        self.assertFalse(self.csv.exists())

    def test_run_creates_and_extends_a_sheet_of_ready_artifacts(self):
        relative = "qc/human_review.csv"
        (self.root / "recipe.spitin").write_text(f"pipeline pipeline.spit\nroot .\nreview made from {relative}\n")
        sheet = self.root / relative
        dag = self.saved()
        code, _, err = call("plan", dag)
        self.assertEqual(code, 0, err)
        self.assertFalse(sheet.exists())
        report = self.run_saved(dag, "--entity", "sub=01")
        self.assertIn(f"record verdicts in {self.root.resolve() / relative}, created by this run", report)
        header, row = list(csv.reader(io.StringIO(sheet.read_text())))
        self.assertEqual(header, ["sub", "@artifact", "@evidence", "verdict", "reason"])
        self.assertEqual(row, ["01", "made[sub=01]", str(self.root.resolve() / "work/01.txt"), "", ""])
        row[0] = "1"
        row[-2:] = ["pass", "checked, by QC"]
        with sheet.open("w", newline="") as output:
            csv.writer(output).writerows([header, row])
        saved = sheet.read_bytes()
        self.run_saved(dag)
        self.assertTrue(sheet.read_bytes().startswith(saved))
        rows = list(csv.reader(io.StringIO(sheet.read_text())))
        self.assertEqual(rows, [header, row, ["02", "made[sub=02]", str(self.root.resolve() / "work/02.txt"), "", ""]])
        self.assertTrue((self.root / "last/01.txt").exists())
        self.assertFalse((self.root / "last/02.txt").exists())
        populated = sheet.read_bytes()
        self.run_saved(dag)
        self.assertEqual(sheet.read_bytes(), populated)

    def test_existing_verdict_file_is_left_as_written(self):
        dag = self.saved()
        existing = b"sub,verdict,reason\r\n01,pass,looked good\r\n"
        self.csv.write_bytes(existing)
        self.run_saved(dag)
        self.assertEqual(self.csv.read_bytes(), existing)

    def test_bypassing_verdicts_does_not_write_a_sheet(self):
        dag = self.saved()
        self.run_saved(dag, "--bypass-verdicts")
        self.assertFalse(self.csv.exists())

    def test_failed_artifacts_are_not_added_to_the_sheet(self):
        dag = self.saved(PIPELINE.replace("cp {input} {@output}", "grep -x 01 {input} > {@output}", 1))
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        rows = list(csv.DictReader(io.StringIO(self.csv.read_text())))
        self.assertEqual([row["@artifact"] for row in rows], ["made[sub=01]"])

    def test_review_sheet_cannot_take_a_planned_output_path(self):
        (self.root / "recipe.spitin").write_text("pipeline pipeline.spit\nroot .\nreview made from work/02.txt\n")
        dag = self.saved()
        code, _, err = call("run", dag, "--entity", "sub=01")
        self.assertEqual(code, 2, err)
        self.assertIn("review file overlaps an artifact", err)
        self.assertFalse((self.root / "work/02.txt").exists())

    def test_generated_row_can_record_a_verdict_without_dimensions(self):
        pipeline = """source raw : Text
operation make(input: Text) -> Text
operation finish(input: Text) -> Text
command make: cp {input} {@output}
command finish: cp {input} {@output}
made = make(raw) @review
last = finish(made)
path raw: data/01.txt
path made: made.txt
path last: last.txt
"""
        dag = self.saved(pipeline)
        self.run_saved(dag)
        header, row = list(csv.reader(io.StringIO(self.csv.read_text())))
        self.assertEqual(header, ["@artifact", "@evidence", "verdict", "reason"])
        self.assertEqual(row[0], "made")
        row[-2] = "pass"
        with self.csv.open("w", newline="") as output:
            csv.writer(output).writerows([header, row])
        self.run_saved(dag)
        self.assertTrue((self.root / "last.txt").exists())

    def test_a_recipe_named_through_a_symlink_reports_one_spelling_of_the_root(self):
        link = self.root.parent / f"{self.root.name}-link"
        link.symlink_to(self.root)
        self.addCleanup(link.unlink)
        spit = os.environ.get("SPIT")
        if not spit:
            self.skipTest("set SPIT to the compiler")
        (self.root / "pipeline.spit").write_text(PIPELINE)
        dag = str(self.root / "jobs.spitdag")
        process = subprocess.run([spit, "dag", str(link / "recipe.spitin"), "-o", dag], capture_output=True, text=True)
        self.assertEqual(process.returncode, 0, process.stderr)
        real = self.root.resolve()
        err = self.run_saved(dag)
        code, out, plan_err = call("plan", dag, "--json")
        self.assertEqual(code, 0, plan_err)
        self.assertIn(f"record verdicts in {real / 'qc.csv'}", err)
        self.assertIn(f"ready to inspect: {real / 'work'}", err)
        self.assertEqual(json.loads(out)["reviews"][0]["file"], str(real / "qc.csv"))
        for text in (err, out, plan_err):
            self.assertNotIn(str(link), text)

    def swap_verdicts(self):
        dag = self.saved()
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.csv.write_text("sub,verdict\n01,fail\n02,pass\n")
        code, out, err = call("plan", dag)
        self.assertEqual(code, 0, err)
        return dag, out + err

    def explain_merge(self, dag):
        code, out, _ = call("plan", dag, "--json")
        merge = next(job["id"] for job in json.loads(out)["jobs"] if job["operation"] == "merge")
        code, out, err = call("explain", str(merge), dag)
        self.assertEqual(code, 0, err)
        return out

    def test_a_rejected_job_is_still_in_the_dag_and_its_aggregate_reads_other_artifacts(self):
        dag, planned = self.swap_verdicts()
        self.assertNotIn("no longer in the DAG", planned)
        self.assertIn("not the artifacts the record holds, so no file is compared with it", self.explain_merge(dag))

    def test_a_rejected_jobs_record_is_not_handed_to_a_job_with_the_same_bytes(self):
        for sub in ("01", "02"):
            (self.root / f"data/{sub}.txt").write_text("same\n")
        dag, planned = self.swap_verdicts()
        self.assertNotIn("recorded at other paths", planned)
        out = self.explain_merge(dag)
        self.assertNotIn("only touched", out)
        self.assertIn("not the artifacts the record holds", out)
        self.run_saved(dag)
        self.csv.write_text("sub,verdict\n01,pass\n02,pass\n")
        code, out, err = call("plan", dag)
        self.assertEqual(code, 0, err)
        self.assertIn("1 job can start", out)

    def test_same_dag_resumes_and_changes_cached_paired_membership(self):
        dag = self.saved()
        code, out, _ = call("plan", dag, "--json")
        self.assertEqual(code, 0, out)
        self.assertEqual(len(json.loads(out)["reviews"][0]["targets"]), 2)
        err = self.run_saved(dag)
        self.assertIn("ready to inspect", err)
        self.assertFalse((self.root / "total.txt").exists())
        self.csv.write_text("sub,verdict,reason\n01,pass,\n02,pass,\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01,02", "01", "02", "01", "02"])
        self.csv.write_text("sub,verdict,reason\n01,pass,\n02,fail,blurred\n")
        self.assertIn("not required", self.run_saved(dag))
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertIn("0 done", self.run_saved(dag))

    def test_a_screen_withholds_a_calculation_and_its_consumer(self):
        dag = self.saved(
            """\
screen usable: grep -qx 01 {@path}
source raw : Scalar .txt [sub]
operation double(value: Scalar @screen(usable)) -> Scalar .txt
calculate double: value * 2
operation finish(value: Scalar) -> Text .txt
command finish: printf '%s\\n' @scalar(value) > {@output}
calculated = double(raw)
final = finish(calculated)
path raw: data/{sub}.txt
path calculated: calculated/{sub}.txt
path final: final/{sub}.txt
""",
            reviewed=(),
        )
        self.run_saved(dag)
        self.assertEqual((self.root / "calculated/01.txt").read_bytes(), b"2.0\n")
        self.assertEqual((self.root / "final/01.txt").read_bytes(), b"2.0\n")
        self.assertFalse((self.root / "calculated/02.txt").exists())
        self.assertFalse((self.root / "final/02.txt").exists())

    def test_bypass_runs_through_missing_or_malformed_verdicts(self):
        dag = self.saved()
        self.csv.write_text("sub,verdict\n01,typo\n")
        code, _, err = call("plan", dag)
        self.assertEqual(code, 2)
        self.assertIn("qc.csv line 2", err)
        self.assertIn("verdicts bypassed", self.run_saved(dag, "--bypass-verdicts"))
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertTrue((self.root / "total.txt").exists())
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)

    def test_clean_keeps_verdict_files_and_rejected_potential_outputs(self):
        recipe = self.root / "recipe.spitin"
        recipe.write_text(recipe.read_text().replace("from qc.csv", "from last/qc.csv"))
        pipeline = ("screen usable: grep -qx 01 {@path}\n" + PIPELINE).replace(
            "operation finish(input: Text) -> Text", "operation finish(input: Text) -> Text @screen(usable)"
        )
        dag = self.saved(pipeline)
        self.run_saved(dag, "--bypass-verdicts")
        last = self.root / "last"
        review = last / "qc.csv"
        review.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        record = last / "last.screens.csv"
        rejected = last / "02.txt"
        self.assertTrue(record.is_file())
        self.assertTrue(rejected.is_file())
        stray = last / "stray.txt"
        stray.write_text("unowned\n")
        for flags in ((), ("--yes",)):
            code, out, err = call("clean", dag, str(last), *flags)
            self.assertEqual(code, 0, err)
            self.assertIn("1 path", out)
            self.assertIn("stray.txt", out)
            for protected in (record, rejected, review):
                self.assertNotIn(protected.name, out)
                self.assertTrue(protected.is_file())
        self.assertFalse(stray.exists())

    def test_bare_relative_screen_script_invalidates_both_cached_verdicts(self):
        pipeline = (
            ("screen usable: python3 decide.py {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            )
        )
        dag = self.saved(pipeline, reviewed=())
        script = self.root / "decide.py"
        record = self.root / "work/made.screens.csv"
        total = self.root / "total.txt"
        script.write_text("raise SystemExit(1)\n")
        self.assertIn("screen `usable` rejected", self.run_saved(dag))
        self.assertFalse(total.exists())
        script.write_text("raise SystemExit(0)\n")
        self.run_saved(dag)
        self.assertIn("01,usable,pass,", record.read_text())
        self.assertEqual(total.read_text().splitlines(), ["01,02", "01", "02", "01", "02"])
        script.write_text("raise SystemExit(1)\n")
        self.assertIn("screen `usable` rejected", self.run_saved(dag))
        self.assertIn("01,usable,fail,", record.read_text())
        self.assertTrue(total.is_file())

    def test_screen_can_reject_a_pair_held_by_review_on_the_other_port(self):
        pipeline = ("screen usable: grep -qx 01 {@path}\n" + PIPELINE).replace(
            "merge(first: many Text, second: many Text)",
            "merge(first: many Text, second: many Text @screen(usable))",
        )
        dag = self.saved(pipeline)
        self.csv.write_text("sub,verdict\n01,pass\n")
        for _ in range(2):
            self.assertIn("screen `usable` rejected", self.run_saved(dag))
            self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)
        (self.root / "data/02.txt").write_text("01\n")
        self.assertIn("held for review", self.run_saved(dag))
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)

    def test_collection_arguments_cannot_name_a_different_member(self):
        pipeline = PIPELINE.replace(
            '@join(first.sub, ",") {first.sub} {second.sub}',
            "{first} @join(first) @list(first)",
        )
        dag = Path(self.saved(pipeline))
        original = dag.read_bytes()
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        for kind in ("member", "join", "list"):
            with self.subTest(kind=kind):
                data = json.loads(original)
                merge = next(job for job in data["jobs"] if job["operation"] == "merge")
                part = next(
                    part
                    for arg in merge["commands"][0]["command"]
                    for part in arg
                    if isinstance(part, dict) and kind in part
                )
                if kind == "member":
                    part[kind]["path"] = "work/02.txt"
                else:
                    part[kind][0] = "work/02.txt"
                dag.write_text(json.dumps(data))
                code, _, err = call("run", str(dag))
                self.assertEqual(code, 2, err)
                self.assertIn(f"job {merge['id']}", err)
                self.assertIn("port 0 offset 0", err)
                self.assertIn("work/02.txt", err)
                self.assertIn("work/01.txt", err)
                self.assertFalse((self.root / "total.txt").exists())

    def test_automatic_screen_reuses_verdict_and_accepts_a_human_override(self):
        (self.root / "data/03.txt").write_text("03\n")
        pipeline = ("screen usable: grep -qx 01 {@path}\n" + PIPELINE).replace(
            "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
        )
        dag = self.saved(pipeline)
        self.assertIn("screen `usable` rejected", self.run_saved(dag))
        record = self.root / "work/made.screens.csv"
        self.assertIn("02,usable,fail,exit status 1", record.read_text())

        def launches():
            logs = (self.root / ".spit-bash/runs").glob("*/jobs/**/*.log")
            return sum(log.read_text().count("$ grep -qx 01 ") for log in logs)

        self.assertEqual(launches(), 3)
        self.assertIn("0 done", self.run_saved(dag))
        self.assertEqual(launches(), 3)
        record.unlink()
        self.assertIn("0 done", self.run_saved(dag))
        self.assertEqual(launches(), 6)
        self.csv.write_text("sub,verdict\n02,pass\n")
        report = self.run_saved(dag)
        self.assertIn("screen `usable` rejected", report)
        self.assertTrue((self.root / "last/01.txt").exists())
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertFalse((self.root / "last/03.txt").exists())
        self.assertIn("02,usable,reviewed", record.read_text())
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01,02", "01", "02", "01", "02"])

    def test_no_upstream_requires_the_producer_of_a_pending_screen(self):
        pipeline = ("screen usable: grep -qx 01 {@path}\n" + PIPELINE).replace(
            "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
        )
        dag = self.saved(pipeline)
        (self.root / "work").mkdir()
        (self.root / "work/02.txt").write_text("02\n")
        args = (dag, "--product", "last", "--entity", "sub=02", "--no-upstream")
        for action in ("plan", "run"):
            with self.subTest(action=action):
                code, _, err = call(action, *args)
                self.assertEqual(code, 2, err)
                self.assertIn("needs a screen verdict from an excluded producer", err)
                self.assertFalse((self.root / "last/02.txt").exists())
        self.csv.write_text("sub,verdict\n02,pass\n")
        self.run_saved(dag, *args[1:])
        self.assertEqual((self.root / "last/02.txt").read_text(), "02\n")

    def test_missing_paired_input_only_blocks_a_required_member(self):
        (self.root / "extra").mkdir()
        (self.root / "extra/01.txt").write_text("01\n")
        pipeline = "source extra : Text [sub]\npath extra: extra/{sub}.txt\n" + (
            PIPELINE.replace("other = finish(raw)", "other = finish(extra)")
        )
        dag = self.saved(pipeline)
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)
        self.csv.write_text("sub,verdict\n01,pass\n02,pass\n")
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("nothing was run", err)

    def test_a_missing_mate_over_two_dimensions_varied_in_reverse_blocks_only_its_member(self):
        (self.root / "extra").mkdir()
        for sub in ("01", "02"):
            for ses in ("a", "b"):
                (self.root / f"data/{sub}_{ses}.txt").write_text(sub + "\n")
                if (sub, ses) != ("02", "b"):
                    (self.root / f"extra/{sub}_{ses}.txt").write_text(ses + "\n")
        pipeline = (
            "source raw : Text [sub, ses]\n"
            "source extra : Text [sub, ses]\n"
            "operation copy_raw(input: Text) -> Text\n"
            "operation finish(input: Text) -> Text\n"
            "operation merge(first: many Text, second: many Text) -> Text\n"
            "command copy_raw: cp {input} {@output}\n"
            "command finish: cp {input} {@output}\n"
            'command merge: printf "%s\u005c\u005cn" {first.sub} {second.ses} > {@output}\n'
            "made = copy_raw(raw) @review\n"
            "other = finish(extra)\n"
            "total = merge(made @vary(sub, ses), other @vary(ses, sub))\n"
            "path raw: data/{sub}_{ses}.txt\n"
            "path extra: extra/{sub}_{ses}.txt\n"
            "path made: work/{sub}_{ses}.txt\n"
            "path other: other/{sub}_{ses}.txt\n"
            "path total: total.txt\n"
        )
        dag = self.saved(pipeline)
        jobs = json.loads(Path(dag).read_text())["jobs"]
        merge = next(job for job in jobs if job.get("collection"))
        self.assertEqual(merge["collection"]["varied"], ["sub", "ses"])
        self.assertEqual([gap["member"] for gap in merge["missing"]], [["02", "b"]])
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01", "01", "a", "b"])
        self.csv.write_text("sub,verdict\n01,pass\n02,pass\n")
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("nothing was run", err)

    def test_partial_run_does_not_screen_a_blocked_producers_old_output(self):
        pipeline = (
            ("screen usable: test -s {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            )
        )
        dag = self.saved(pipeline, reviewed=())
        self.run_saved(dag)
        old = self.root / "last/02.txt"
        old.unlink()
        (self.root / "data/02.txt").unlink()
        code, _, err = call("run", dag, "--partial")
        self.assertEqual(code, 1, err)
        self.assertFalse(old.exists(), err)

    def test_screen_rejection_drops_a_paired_members_blocked_producer(self):
        pipeline = (
            ("screen usable: grep -qx 01 {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            )
            .replace(
                "other = finish(raw)",
                "other = finish(extra)",
            )
        )
        (self.root / "extra").mkdir()
        for sub in ("01", "02"):
            (self.root / f"extra/{sub}.txt").write_text(sub + "\n")
        dag = self.saved("source extra : Text [sub]\npath extra: extra/{sub}.txt\n" + pipeline, reviewed=())
        (self.root / "extra/02.txt").unlink()
        code, _, err = call("run", dag, "--partial")
        self.assertEqual(code, 1, err)
        self.assertIn("screen `usable` rejected", err)
        self.assertTrue((self.root / "total.txt").exists(), err)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)

    def test_rejected_work_does_not_require_its_executable(self):
        pipeline = (
            ("screen usable: grep -qx never {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            )
            .replace(
                "last = finish(made)",
                "operation rejected(input: Text) -> Text\n"
                "command rejected: missing-screened-job-command {input} {@output}\n"
                "last = rejected(made)",
            )
        )
        dag = self.saved(pipeline, reviewed=())
        self.assertIn("not required", self.run_saved(dag))
        self.assertFalse((self.root / "last/01.txt").exists())
        self.assertTrue((self.root / "other/01.txt").exists())

    def test_reuse_distinguishes_two_different_members_with_identical_bytes(self):
        for sub in ("01", "02"):
            (self.root / f"data/{sub}.txt").write_text("same\n")
        pipeline = PIPELINE.replace('@join(first.sub, ",") {first.sub} {second.sub}', "{first}")
        dag = self.saved(pipeline)
        for accepted, rejected in (("01", "02"), ("02", "01")):
            self.csv.write_text(f"sub,verdict\n{accepted},pass\n{rejected},fail\n")
            self.run_saved(dag)
            self.assertEqual((self.root / "total.txt").read_text(), f"work/{accepted}.txt\n")

    def test_review_chain_can_run_ahead_in_one_invocation(self):
        def step(index):
            prior = "raw" if index == 1 else f"p{index - 1}"
            return f"p{index} = gated({prior}) @review\npath p{index}: p{index}/{{sub}}.txt\n"

        pipeline = """source raw : Text [sub]
operation gated(input: Text) -> Text
command gated: cp {input} {@output}
path raw: data/{sub}.txt
""" + "".join(step(index) for index in range(1, 7))
        dag = self.saved(pipeline, reviewed=tuple(f"p{index}" for index in range(1, 7)))
        self.run_saved(dag, "--bypass-verdicts")
        self.assertTrue((self.root / "p6/02.txt").exists())
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.assertIn(" failed, 5 not required, ", self.run_saved(dag))
        self.assertTrue((self.root / "p6/02.txt").exists())

    def test_one_call_attaches_checks_and_screens(self):
        (self.root / "recipe.spitin").write_text("pipeline pipeline.spit\nroot .\n")
        pipeline = """check sound: test -s {@path}
screen usable: grep -qx 01 {@path}
source raw : Text [sub]
operation copy_raw(input: Text) -> Text
command copy_raw: cp {input} {@output}
gated = copy_raw(raw @check(sound) @screen(usable))
other = copy_raw(raw)
path raw: data/{sub}.txt
path gated: gated/{sub}.txt
path other: other/{sub}.txt
"""
        dag = self.saved(pipeline)
        jobs = json.loads(Path(dag).read_text())["jobs"]
        self.assertEqual([len(job["checks"]) for job in jobs], [1, 1, 0, 0])
        self.run_saved(dag)
        self.assertTrue((self.root / "gated/01.txt").is_file())
        self.assertFalse((self.root / "gated/02.txt").exists())
        self.assertTrue((self.root / "other/02.txt").is_file())

    def test_screen_launch_failure_fails_execution(self):
        dag = self.saved(
            ("screen usable: missing-screen-command {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            ),
            reviewed=(),
        )
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("screen usable", err)
        self.assertNotIn("screen `usable` rejected", err)
        self.assertFalse((self.root / "total.txt").exists())

    def test_a_signal_is_an_execution_failure_rather_than_a_verdict(self):
        declaration = "screen usable: python3 -c 'import os, signal; os.kill(os.getpid(), signal.SIGTERM)' {@path}"
        dag = self.saved(
            (declaration + "\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            ),
            reviewed=(),
        )
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("command terminated by signal 15", err)
        self.assertFalse((self.root / "work/made.screens.csv").exists())

    def test_runtime_screens_fail_closed_on_slurm(self):
        dag = self.saved(
            ("screen usable: test -s {@path}\n" + PIPELINE)
            .replace(" @review", "")
            .replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @screen(usable)"
            ),
            reviewed=(),
        )
        code, _, err = call("run", dag, "--executor", "slurm")
        self.assertEqual(code, 2)
        self.assertIn("require --executor local", err)
        self.assertFalse((self.root / "work/01.txt").exists())

    def test_bypass_still_runs_artifact_checks(self):
        pipeline = "check sound: test ! -s {@path}\n" + PIPELINE
        dag = self.saved(
            pipeline.replace(
                "operation copy_raw(input: Text) -> Text", "operation copy_raw(input: Text) -> Text @check(sound)"
            )
        )
        code, _, err = call("run", dag, "--bypass-verdicts")
        self.assertEqual(code, 1, err)
        self.assertIn("check sound", err)
        self.assertFalse((self.root / "last/01.txt").exists())


if __name__ == "__main__":
    unittest.main()
