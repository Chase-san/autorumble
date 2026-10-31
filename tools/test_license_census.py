#!/usr/bin/env python3
"""Tests for license_census.py. Run with: python3 tools/test_license_census.py"""
import pathlib
import tempfile
import unittest
import zipfile

import license_census as census

PROPERTIES = "robot.classname=me.Bot\nrobot.version=1.0\nrobot.author.name=Me\n"
RWPCL = "// Released under the RWPCL\n"
ZLIB = "// Permission is granted to anyone to use this software for any purpose,\n"
BOT_SOURCE = "package me;\npublic class Bot extends Robot {}\n"
HELPER_SOURCE = "package me;\nclass Helper {}\n"


def make_jar(directory, files):
    path = pathlib.Path(directory) / "me.Bot_1.0.jar"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("me/Bot.properties", PROPERTIES)
        for name, text in files.items():
            archive.writestr(name, text)
    return path


def verdict(files, page=None):
    with tempfile.TemporaryDirectory() as directory:
        jar = census.Jar(make_jar(directory, files))
    pages = {"Bot": page} if page is not None else {}
    wiki = census.Wiki("me.Bot", jar.author, pages)
    return census.decide_verdict(jar, wiki)


COMPLETE = {"me/Bot.class": "", "me/Bot.java": BOT_SOURCE, "me/Helper.class": "", "me/Helper.java": HELPER_SOURCE}


def with_files(**changes):
    files = dict(COMPLETE)
    for name, text in changes.items():
        files[name.replace("__", "/").replace("_dot_", ".")] = text
    return files


def page(license_field="", extra="", author="Me"):
    return f"{{{{Infobox Robot\n| author = {author}\n| license = {license_field}\n}}}}\nAbout me.Bot.\n{extra}"


class SourceTest(unittest.TestCase):
    def test_no_source(self):
        self.assertEqual(verdict({"me/Bot.class": ""})[0], "no-source")

    def test_some_own_source(self):
        files = {"me/Bot.class": "", "me/Other.class": "", "me/Bot.java": RWPCL + BOT_SOURCE}
        self.assertEqual(verdict(files)[0], "partial-source")

    def test_a_class_declared_in_another_file_has_source(self):
        files = {"me/Bot.class": "", "me/Other.class": "", "me/Bot.java": RWPCL + BOT_SOURCE + "class Other {}\n"}
        self.assertEqual(verdict(files)[0], "eligible")

    def test_a_library_without_source_goes_to_review(self):
        files = with_files(me__Bot_dot_java=RWPCL + BOT_SOURCE, lib__Tree_dot_class="")
        self.assertEqual(verdict(files)[0], "review-library-source")


class JarLicenseTest(unittest.TestCase):
    def test_source_without_a_license_is_out(self):
        self.assertEqual(verdict(COMPLETE)[0], "no-license")

    def test_copyright_alone_is_not_a_license(self):
        files = with_files(me__Bot_dot_java="// Copyright (c) 2020 Me\n" + BOT_SOURCE)
        self.assertEqual(verdict(files)[0], "no-license")

    def test_a_license_in_the_main_class_counts(self):
        self.assertEqual(verdict(with_files(me__Bot_dot_java=RWPCL + BOT_SOURCE))[:2], ("eligible", "RWPCL"))

    def test_a_license_file_counts(self):
        self.assertEqual(verdict(with_files(LICENSE="The MIT License\n"))[:2], ("eligible", "MIT"))

    def test_someone_elses_license_on_embedded_code_does_not_count(self):
        files = with_files(ags__KdTree_dot_java=ZLIB + "class KdTree {}\n", ags__KdTree_dot_class="")
        self.assertEqual(verdict(files)[0], "no-license")

    def test_a_license_on_one_of_many_own_files_is_weak(self):
        files = dict(COMPLETE)
        for number in range(4):
            files[f"me/Part{number}.java"] = f"class Part{number} {{}}\n"
            files[f"me/Part{number}.class"] = ""
        files["me/Part0.java"] = ZLIB + "class Part0 {}\n"
        self.assertEqual(verdict(files)[0], "review-weak")

    def test_words_alone_are_not_a_license(self):
        files = with_files(me__Bot_dot_java="// Compressed with zlib. npl and kpl are counters.\n" + BOT_SOURCE)
        self.assertEqual(verdict(files)[0], "no-license")

    def test_plain_permission_goes_to_review(self):
        files = with_files(me__Bot_dot_java="// Feel free to use this code.\n" + BOT_SOURCE)
        self.assertEqual(verdict(files)[0], "review-informal")


class DisallowedTest(unittest.TestCase):
    def test_kpl_anywhere_is_out(self):
        files = with_files(me__Bot_dot_java=RWPCL + BOT_SOURCE,
                           kawigi__Gun_dot_java="// released under the terms of the KPL\nclass Gun {}\n",
                           kawigi__Gun_dot_class="")
        self.assertEqual(verdict(files)[:2], ("disallowed", "KPL"))

    def test_nat_license_is_out(self):
        files = with_files(me__Bot_dot_java="// Nat Public Robot License 2.1\n" + BOT_SOURCE)
        self.assertEqual(verdict(files)[:2], ("disallowed", "NPL"))

    def test_a_competition_ban_is_out(self):
        text = "// Use in any programming competition without the written permission of the author is forbidden.\n"
        files = with_files(me__Bot_dot_java=ZLIB + text + BOT_SOURCE)
        self.assertEqual(verdict(files)[:2], ("disallowed", "competition-ban"))

    def test_gpl_needs_a_ruling(self):
        files = with_files(me__Bot_dot_java="// under the terms of the GNU General Public License\n" + BOT_SOURCE)
        self.assertEqual(verdict(files)[:2], ("decide", "GPL"))


class WikiTest(unittest.TestCase):
    def test_a_wiki_license_is_enough(self):
        self.assertEqual(verdict(COMPLETE, page("[[RWPCL]]"))[:3], ("eligible", "RWPCL", "wiki"))

    def test_the_can_i_use_your_code_answer_counts(self):
        extra = "; Can I use your code?\n: Yes, it's under the [http://x zlib license].\n; Next?\n: RWPCL talk.\n"
        self.assertEqual(verdict(COMPLETE, page(extra=extra))[:2], ("eligible", "zlib"))

    def test_the_wiki_can_rule_a_robot_out(self):
        self.assertEqual(verdict(COMPLETE, page("[[KPL]]"))[:2], ("disallowed", "KPL"))

    def test_a_competition_ban_in_a_friendly_answer_is_out(self):
        extra = ("==== Can I use your code? ====\nSure, but under the following terms:\n*Give credit.\n"
                 "*Use of this code without my personal permission in ANY competition WHATSOEVER is strictly "
                 "prohibited.\n")
        self.assertEqual(verdict(COMPLETE, page(extra=extra))[:2], ("disallowed", "competition-ban"))

    def test_a_license_only_in_the_page_text_is_weak(self):
        extra = "It uses a gun released under the RWPCL.\n"
        self.assertEqual(verdict(COMPLETE, page(extra=extra))[:2], ("review-weak", "RWPCL"))

    def test_a_banned_license_in_the_page_text_goes_to_review(self):
        extra = "It borrows from a robot under the Kawigi Public License.\n"
        self.assertEqual(verdict(COMPLETE, page("MIT", extra))[0], "review-conflict")

    def test_share_alike_needs_a_ruling(self):
        self.assertEqual(verdict(COMPLETE, page("Share-Alike"))[:2], ("decide", "CC-BY-SA"))

    def test_no_derivatives_needs_a_ruling(self):
        self.assertEqual(verdict(COMPLETE, page("[http://x CC BY-ND 3.0]"))[:2], ("decide", "CC-BY-ND"))

    def test_attribution_on_the_wiki_is_allowed(self):
        self.assertEqual(verdict(COMPLETE, page("Attribution"))[:2], ("eligible", "Attribution"))

    def test_freebsd_is_bsd(self):
        self.assertEqual(verdict(COMPLETE, page("FreeBSD"))[:2], ("eligible", "BSD"))

    def test_an_unknown_license_name_goes_to_review(self):
        self.assertEqual(verdict(COMPLETE, page("GCWCD"))[0], "review-informal")

    def test_the_wiki_permission_beats_a_license_on_an_embedded_tree(self):
        files = dict(COMPLETE)
        for number in range(4):
            files[f"me/Part{number}.java"] = f"class Part{number} {{}}\n"
            files[f"me/Part{number}.class"] = ""
        files["me/Part0.java"] = ZLIB + "class Part0 {}\n"
        self.assertEqual(verdict(files, page("no strings attached"))[0], "review-informal")

    def test_a_jar_conflict_goes_to_review(self):
        files = with_files(me__Bot_dot_java=RWPCL + BOT_SOURCE)
        self.assertEqual(verdict(files, page("[[NPRL]]"))[0], "review-conflict")
        files = with_files(me__Bot_dot_java="// the GNU General Public License\n" + BOT_SOURCE)
        self.assertEqual(verdict(files, page("MIT"))[0], "review-conflict")

    def test_rights_held_back_in_the_jar_conflict_with_the_wiki(self):
        files = with_files(me__Bot_dot_java="// Copyright Me. All rights reserved.\n" + BOT_SOURCE)
        self.assertEqual(verdict(files, page("MIT"))[0], "review-conflict")

    def test_a_bsd_notice_is_not_rights_held_back(self):
        notice = "// All rights reserved. Redistribution and use in source and binary forms are permitted.\n"
        files = with_files(me__Bot_dot_java=notice + BOT_SOURCE)
        self.assertEqual(verdict(files, page("RWPCL"))[0], "eligible")

    def test_the_package_in_the_infobox_author_identifies_the_page(self):
        other = "{{Infobox Robot\n| author = [[User:Me2|Someone]]\n| license = RWPCL\n}}\nA robot.\n"
        with tempfile.TemporaryDirectory() as directory:
            jar = census.Jar(make_jar(directory, COMPLETE))
        self.assertFalse(census.Wiki("me.Bot", "", {"Bot": other}).verified)
        self.assertTrue(census.Wiki("sheldor.Bot", "", {"Bot": other.replace("Me2", "Sheldor2")}).verified)
        self.assertTrue(jar.readable)

    def test_two_allowed_licenses_do_not_conflict(self):
        files = with_files(me__Bot_dot_java=ZLIB + BOT_SOURCE)
        self.assertEqual(verdict(files, page("RWPCL"))[0], "eligible")

    def test_closed_on_the_wiki_conflicts_with_a_license_in_the_jar(self):
        files = with_files(me__Bot_dot_java=RWPCL + BOT_SOURCE)
        self.assertEqual(verdict(files, page("None"))[0], "review-conflict")

    def test_a_page_about_another_robot_does_not_count(self):
        other = "{{Infobox Robot\n| author = Someone\n| license = RWPCL\n}}\nAbout other.Bot.\n"
        self.assertEqual(verdict(COMPLETE, other)[0], "review-wiki-page")

    def test_titles_are_capitalized_like_the_wiki(self):
        wiki = census.Wiki("eem.zapper", "", {"Zapper": page("MIT")})
        self.assertEqual(wiki.page, "Zapper")

    def test_a_disambiguated_title_is_found(self):
        wiki = census.Wiki("sample.Walls", "", {"Walls (robot)": page("MIT")})
        self.assertEqual(wiki.page, "Walls (robot)")

    def test_redirects_are_followed(self):
        with tempfile.TemporaryDirectory() as directory:
            jar = census.Jar(make_jar(directory, COMPLETE))
        pages = {"Bot": "#REDIRECT [[Bot Page]]", "Bot Page": page("MIT")}
        wiki = census.Wiki("me.Bot", jar.author, pages)
        self.assertEqual(census.decide_verdict(jar, wiki)[:2], ("eligible", "MIT"))


class ListTest(unittest.TestCase):
    def test_reads_lists_and_remembers_which(self):
        with tempfile.TemporaryDirectory() as directory:
            first = pathlib.Path(directory) / "duel.txt"
            second = pathlib.Path(directory) / "melee.txt"
            first.write_text("<pre>\na.One 1.0,http://x/a.jar\nnot an entry\n</pre>\n")
            second.write_text("a.One 1.0,http://x/a.jar\nb.Two 2,http://x/b.jar\n")
            robots = census.read_lists([first, second])
        self.assertEqual(robots["a.One 1.0"]["lists"], {"duel", "melee"})
        self.assertEqual(robots["b.Two 2"]["url"], "http://x/b.jar")
        self.assertEqual(len(robots), 2)


if __name__ == "__main__":
    unittest.main()
