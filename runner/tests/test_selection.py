import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.selection import Selection, parse_entities, parse_products, parse_stage, parse_stages, select
from support import document, job_entry, read, UPPER


def made(data, id, product, entities, stage=()):
    output = data["jobs"][id - 1]["outputs"]["output"]
    output["product"] = product
    output["entities"] = entities
    data["jobs"][id - 1]["stage"] = list(stage)


def study(root):
    """A template, a per-subject step that reads it, a stage inside per subject, and a group mean."""
    data = document(root, [
        job_entry(1, ["source.txt"], "template.txt", UPPER, dependents=[2, 3]),
        job_entry(2, ["template.txt"], "sub-01.txt", UPPER, depends_on=[1], dependents=[4, 5]),
        job_entry(3, ["template.txt"], "sub-02.txt", UPPER, depends_on=[1], dependents=[6]),
        job_entry(4, ["sub-01.txt"], "sub-01-clean.txt", UPPER, depends_on=[2], dependents=[7]),
        job_entry(5, ["sub-01.txt"], "sub-01-qc.txt", UPPER, depends_on=[2]),
        job_entry(6, ["sub-02.txt"], "sub-02-clean.txt", UPPER, depends_on=[3], dependents=[7]),
        job_entry(7, ["sub-01-clean.txt", "sub-02-clean.txt"], "mean.txt", UPPER, depends_on=[4, 6]),
    ], ["source.txt"])
    made(data, 1, "template", {})
    made(data, 2, "aligned", {"sub": "01"}, ["pre"])
    made(data, 3, "aligned", {"sub": "02"}, ["pre"])
    made(data, 4, "clean", {"sub": "01"}, ["pre", "denoise"])
    made(data, 5, "qc", {"sub": "01"})
    made(data, 6, "clean", {"sub": "02"}, ["pre", "denoise"])
    made(data, 7, "mean", {})
    return data


class SelectionTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.dag = read(study(Path(directory.name)))

    def ids(self, **criteria):
        return [job.id for job in select(self.dag, Selection(**criteria)).dag.jobs]

    def test_no_criteria_keeps_everything(self):
        self.assertEqual(self.ids(), [1, 2, 3, 4, 5, 6, 7])

    def test_entities_keep_matching_jobs_and_what_they_need_but_nothing_downstream(self):
        selected = select(self.dag, Selection(entities=(parse_entities("sub=01"),)))
        self.assertEqual([job.id for job in selected.dag.jobs], [1, 2, 4, 5])
        self.assertEqual((selected.matched, selected.needed), (3, 1))
        self.assertEqual(selected.matched_ids, frozenset({2, 4, 5}))
        self.assertEqual(self.ids(entities=(parse_entities("sub=01"), parse_entities("sub=02"))), [1, 2, 3, 4, 5, 6])

    def test_product_and_stage_combine_with_entities(self):
        self.assertEqual(self.ids(products=("clean",)), [1, 2, 3, 4, 6])
        self.assertEqual(self.ids(products=("clean",), entities=(parse_entities("sub=02"),)), [1, 3, 6])
        self.assertEqual(self.ids(stages=(parse_stage("pre"),)), [1, 2, 3, 4, 6])
        self.assertEqual(self.ids(stages=(parse_stage("pre/denoise"),), entities=(parse_entities("sub=01"),)), [1, 2, 4])
        self.assertEqual(self.ids(products=("mean",)), [1, 2, 3, 4, 6, 7])

    def test_value_list_selects_the_same_jobs_as_repeated_flags(self):
        shorthand = select(self.dag, Selection(entities=(parse_entities("sub=01,02"),)))
        repeated = select(self.dag, Selection(entities=(parse_entities("sub=01"), parse_entities("sub=02"))))
        self.assertEqual(shorthand, repeated)

    def test_product_and_stage_lists_select_the_same_jobs_as_repeated_flags(self):
        cases = [
            (Selection(products=parse_products("qc,mean")),
             Selection(products=(*parse_products("qc"), *parse_products("mean")))),
            (Selection(stages=parse_stages("pre/denoise,other")),
             Selection(stages=(*parse_stages("pre/denoise"), *parse_stages("other")))),
        ]
        data = study(self.dag.root)
        made(data, 5, "qc", {"sub": "01"}, ["other"])
        dag = read(data)
        for listed, repeated in cases:
            with self.subTest(listed):
                self.assertEqual(listed, repeated)
                self.assertEqual(select(dag, listed), select(dag, repeated))
        self.assertEqual(select(dag, cases[0][0]).matched_ids, {5, 7})
        self.assertEqual(select(dag, cases[1][0]).matched_ids, {4, 5, 6})

    def test_lists_combine_with_entities(self):
        self.assertEqual(self.ids(products=parse_products("clean,qc"), entities=(parse_entities("sub=01"),)),
                         [1, 2, 4, 5])
        self.assertEqual(self.ids(stages=parse_stages("pre/denoise,pre"), entities=(parse_entities("sub=02"),)),
                         [1, 3, 6])

    def test_value_lists_match_each_dimension_and_repeated_flags_match_specific_pairs(self):
        data = study(self.dag.root)
        made(data, 4, "clean", {"sub": "01", "ses": "baseline"})
        made(data, 5, "qc", {"sub": "01", "ses": "followup"})
        made(data, 6, "clean", {"sub": "02", "ses": "followup"})
        dag = read(data)

        baseline = Selection(entities=(parse_entities("sub=01,02,ses=baseline"),))
        self.assertEqual(select(dag, baseline).matched_ids, {4})
        both = Selection(entities=(parse_entities("sub=01,02,ses=baseline,followup"),))
        self.assertEqual(select(dag, both).matched_ids, {4, 5, 6})
        pairs = Selection(entities=(parse_entities("sub=01,ses=baseline"), parse_entities("sub=02,ses=followup")))
        self.assertEqual(select(dag, pairs).matched_ids, {4, 6})

    def test_names_what_cannot_match(self):
        cases = [
            (Selection(entities=(parse_entities("subject=01"),)), "no output has dimension `subject`; dimensions: sub"),
            (Selection(entities=(parse_entities("sub=03"),)), "no output has sub=03; sub takes 01, 02"),
            (Selection(entities=(parse_entities("sub=01,03"),)), "no output has sub=03; sub takes 01, 02"),
            (Selection(products=("cleaned",)), "products: aligned, clean, mean, qc, template"),
            (Selection(stages=(parse_stage("denoise"),)), "stages: pre, pre/denoise"),
            (Selection(products=parse_products("clean,cleaned")), "no output is of product `cleaned`; products: aligned"),
            (Selection(stages=parse_stages("pre,pre/clean")), "no job is in stage `pre/clean`; stages: pre, pre/denoise"),
            (Selection(products=("qc",), stages=(parse_stage("pre"),)), "no job's outputs match every criterion"),
        ]
        for selection, message in cases:
            with self.subTest(message), self.assertRaisesRegex(DagError, message):
                select(self.dag, selection)

    def test_parsing(self):
        self.assertEqual(parse_entities("ses=2,sub=01,02"), (("ses", {"2"}), ("sub", {"01", "02"})))
        self.assertEqual(parse_entities("run=a=b,c"), (("run", {"a=b", "c"}),))
        for text in ["sub", "=01", "sub=", ",sub=01", "sub=01,sub=02", "sub=01,", "sub=01,,02"]:
            with self.subTest(text), self.assertRaises(ValueError):
                parse_entities(text)
        self.assertEqual(parse_stage("pre/denoise"), ("pre", "denoise"))
        with self.assertRaises(ValueError):
            parse_stage("pre/")
        self.assertEqual(parse_products("clean,mask"), ("clean", "mask"))
        self.assertEqual(parse_stages("pre,pre/denoise"), (("pre",), ("pre", "denoise")))
        for parse, text in [(parse_products, "clean,,mask"), (parse_products, "clean,"), (parse_products, ""),
                            (parse_stages, "pre,"), (parse_stages, ",pre"), (parse_stages, "pre,/denoise")]:
            with self.subTest(text), self.assertRaises(ValueError):
                parse(text)


if __name__ == "__main__":
    unittest.main()
