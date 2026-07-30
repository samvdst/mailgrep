#!/usr/bin/env python3
"""Generate a synthetic demo mailbox (fixture-source layout) for demos and
screenshots. Everything is fictional. Usage: demo-corpus.py <output-dir>"""
import base64
import os
import random
import sys
from datetime import datetime, timedelta, timezone

random.seed(7)
OUT = sys.argv[1] if len(sys.argv) > 1 else "demo-corpus"

PEOPLE = [
    ("Anna Keller", "anna.keller@alpenblick-immobilien.ch"),
    ("Marc Dubois", "marc.dubois@atelier-dubois.ch"),
    ("Lisa Brunner", "l.brunner@treuhand-brunner.ch"),
    ("Jonas Weber", "jonas@weber-sanitaer.ch"),
    ("Sophie Martin", "sophie.martin@gmail.com"),
    ("David Chen", "david.chen@nimbusworks.io"),
    ("Elena Rossi", "elena@studiorossi.it"),
    ("Tom Baker", "tom.baker@brightpixel.dev"),
    ("Petra Vogel", "petra.vogel@stadtwerke-muster.de"),
    ("Nico Meier", "nico@velowerkstatt.ch"),
    ("Claire Fontaine", "claire@fontaine-avocats.ch"),
    ("Ben Okafor", "ben.okafor@quantleaf.dev"),
]
ROBOTS = [
    ("Cloud Invoices", "billing@nimbusworks.io", "invoice"),
    ("Stadtwerke Abrechnung", "rechnung@stadtwerke-muster.de", "invoice"),
    ("Hosting Billing", "billing@serverpark.example", "invoice"),
    ("Alpen Newsletter", "newsletter@alpenblick-immobilien.ch", "news"),
    ("Conference Team", "noreply@rustconf.example", "news"),
]
ME = ("Sam Demo", "sam@demo.example")

THREADS = [
    # (subject, participants, messages: (author_idx or -1=me, body, days_gap))
    ("Heizung Ersatzteil Offerte", [3], [
        (0, "Guten Tag\n\nDie Offerte fuer das Ersatzteil der Heizung liegt bei.\nEinbau waere ab naechster Woche moeglich.\n\nFreundliche Gruesse\nJonas Weber", 0),
        (-1, "Danke fuer die schnelle Offerte!\nDer Preis passt, bitte einplanen.", 1),
        (0, "Perfekt, wir kommen am Dienstag um 8 Uhr.", 2),
    ]),
    ("Mietvertrag Verlaengerung", [0], [
        (0, "Sehr geehrter Herr Demo\n\nIhr Mietvertrag laeuft Ende Jahr aus. Anbei der Entwurf fuer die Verlaengerung um weitere zwei Jahre.\n\nMit freundlichen Gruessen\nAnna Keller", 0),
        (-1, "Guten Tag Frau Keller\n\nBesten Dank. Zwei Punkte: die Nebenkosten scheinen mir hoch, und ich haette gerne eine Klausel fuer den Keller.\n\nGruesse\nSam", 3),
        (0, "Wir haben die Nebenkosten angepasst, neue Version im Anhang.", 6),
    ]),
    ("Website relaunch timeline", [7, 5], [
        (0, "Hey Sam,\n\nsketching the relaunch timeline: content freeze end of month, staging the week after, launch on the 15th. Realistic?", 0),
        (-1, "Works for me. Can we get the search prototype in before content freeze?", 1),
        (1, "I can review the copy next week if that helps the freeze.", 2),
        (0, "Deal. Staging link follows.", 4),
    ]),
    ("Steuererklaerung Unterlagen", [2], [
        (0, "Guten Tag Herr Demo\n\nFuer die Steuererklaerung fehlen noch: Lohnausweis, Saeule 3a Bescheinigung und die Zinsabrechnung.\n\nFreundliche Gruesse\nLisa Brunner", 0),
        (-1, "Anbei der Lohnausweis und die 3a-Bescheinigung. Die Zinsabrechnung folgt.", 2),
    ]),
    ("Rechnung Velo Service", [9], [
        (0, "Hoi Sam\n\nDein Velo ist abholbereit, die Rechnung fuer den Service haengt an. Bremsbelaege waren komplett durch.\n\nGruss Nico", 0),
        (-1, "Super, danke! Ich komme Samstag vorbei und zahle bar.", 1),
    ]),
    ("Nebenkostenabrechnung 2023", [0], [
        (0, "Sehr geehrter Herr Demo\n\nAnbei die Nebenkostenabrechnung 2023. Die Rechnung weist ein Guthaben von CHF 142.50 aus.\n\nFreundliche Gruesse\nAnna Keller", 0),
        (-1, "Besten Dank, das Guthaben duerfen Sie mit der naechsten Miete verrechnen.", 4),
    ]),
    ("Mandat Nachbarschaftsstreit", [10], [
        (0, "Cher Monsieur,\n\nSuite a notre entretien, vous trouverez ci-joint la convention d'honoraires pour le mandat.\n\nMaitre Fontaine", 0),
        (-1, "Merci beaucoup. Je vous renvoie la convention signee cette semaine.", 3),
        (0, "Bien recu, nous deposons la requete lundi.", 8),
    ]),
    ("Search index performance review", [11, 5], [
        (0, "Sam — profiled the new index build: 40% faster after the mmap change, but memory spikes during merges. Thoughts?", 0),
        (1, "We hit the same thing at Nimbus; capping merge threads helped.", 1),
        (-1, "Capping at 2 merge threads now, spike is gone. Shipping it.", 2),
    ]),
]

ONEOFFS = [
    (0, "Besichtigungstermin bestaetigt", "Der Termin fuer die Wohnungsuebergabe ist am Freitag 14:00 bestaetigt."),
    (1, "Devis renovation cuisine", "Bonjour Sam,\n\nVous trouverez ci-joint le devis pour la renovation de la cuisine.\n\nCordialement,\nMarc"),
    (4, "Fotos vom Wochenende", "Hoi Sam!\n\nDie Fotos vom Wanderwochenende sind online — die vom Gipfel sind der Hammer."),
    (5, "Standup notes + search latency", "Notes from today: p99 search latency down to 8ms after the index rebuild. Shipping Thursday."),
    (6, "Preventivo sito web", "Ciao Sam,\n\nin allegato il preventivo per il nuovo sito. Fammi sapere!\n\nElena"),
    (3, "Rechnung Boiler Reparatur", "Guten Tag\n\nDie Rechnung fuer die Boiler-Reparatur vom Montag liegt bei. Der Klempner musste das Ventil komplett ersetzen.\n\nJonas Weber"),
    (8, "Zaehlerstand Ablesung", "Sehr geehrter Kunde,\n\nbitte teilen Sie uns bis Ende Monat Ihren aktuellen Zaehlerstand mit.\n\nStadtwerke Muster"),
    (2, "Fristerstreckung Steuern genehmigt", "Guten Tag Herr Demo\n\nDie Fristerstreckung bis 30. September wurde genehmigt.\n\nLisa Brunner"),
    (7, "Design review Thursday?", "Hey — can we move the design review to Thursday 15:00? The new facet rail mockups are ready."),
    (9, "Winterreifen einlagern", "Hoi Sam\n\nDeine Winterreifen sind eingelagert, Abholung ab Oktober jederzeit moeglich."),
    (10, "Convention signee recue", "Cher Monsieur,\n\nNous confirmons la reception de la convention signee.\n\nMaitre Fontaine"),
    (11, "Benchmark numbers look great", "The comparison table is done — we beat the baseline on every recall metric. Draft attached tomorrow."),
]


def rfc(dt):
    return dt.strftime("%a, %d %b %Y %H:%M:%S +0100")


def write_eml(folder, name, headers, body, date, mtime_now=False):
    d = os.path.join(OUT, folder)
    os.makedirs(d, exist_ok=True)
    path = os.path.join(d, name)
    with open(path, "w") as f:
        f.write(headers + "\r\n\r\n" + body + "\r\n")
    if not mtime_now:
        ts = date.timestamp()
        os.utime(path, (ts, ts))


def base_headers(msgid, subj, frm, to, date, refs=None, attach=None):
    h = [
        f"Message-ID: <{msgid}>",
        f"Received: from mx.example.org by mail.demo.example with ESMTP; {rfc(date)}",
        f"Date: {rfc(date - timedelta(minutes=1))}",
        f"From: {frm[0]} <{frm[1]}>",
        f"To: {to[0]} <{to[1]}>",
        f"Subject: {subj}",
    ]
    if refs:
        h.append("References: " + " ".join(f"<{r}>" for r in refs))
        h.append(f"In-Reply-To: <{refs[-1]}>")
    return h


def plain(headers, body):
    return "\r\n".join(headers + ["Content-Type: text/plain; charset=utf-8"]), body


def with_pdf(headers, body, pdfname):
    fake = base64.b64encode(b"%PDF-1.4 demo attachment for screenshots").decode()
    hdrs = "\r\n".join(headers + ["MIME-Version: 1.0", 'Content-Type: multipart/mixed; boundary="BB"'])
    b = (
        f"--BB\r\n"
        f"Content-Type: text/plain; charset=utf-8\r\n"
        f"\r\n"
        f"{body}\r\n"
        f"--BB\r\n"
        f"Content-Type: application/pdf; name=\"{pdfname}\"\r\n"
        f"Content-Disposition: attachment; filename=\"{pdfname}\"\r\n"
        f"Content-Transfer-Encoding: base64\r\n"
        f"\r\n"
        f"{fake}\r\n"
        f"--BB--"
    )
    return hdrs, b


start = datetime(2021, 3, 1, 9, 0, tzinfo=timezone.utc)
n = 0

for t_i, (subj, ppl, msgs) in enumerate(THREADS):
    refs = []
    base = start + timedelta(days=t_i * 37, hours=t_i * 3)
    for m_i, (author, body, gap) in enumerate(msgs):
        date = base + timedelta(days=gap, hours=m_i)
        frm = ME if author == -1 else PEOPLE[ppl[author % len(ppl)]]
        to = PEOPLE[ppl[0]] if author == -1 else ME
        msgid = f"t{t_i}m{m_i}@demo.example"
        prefix = "" if m_i == 0 else ("AW: " if "ae" in subj or "Miet" in subj or "Steuer" in subj or "Heizung" in subj else "Re: ")
        headers = base_headers(msgid, prefix + subj, frm, to, date, refs[:] or None)
        if m_i == 0 and ("Offerte" in subj or "Mietvertrag" in subj):
            hdrs, bdy = with_pdf(headers, body, subj.split()[0] + ".pdf")
        else:
            hdrs, bdy = plain(headers, body)
        folder = "Sent" if author == -1 else "INBOX"
        n += 1
        write_eml(folder, f"{n:03}.eml", hdrs, bdy, date)
        refs.append(msgid)

for i, (p_i, subj, body) in enumerate(ONEOFFS):
    date = start + timedelta(days=20 + i * 61, hours=i * 5)
    headers = base_headers(f"one{i}@demo.example", subj, PEOPLE[p_i], ME, date)
    if "devis" in subj.lower() or "preventivo" in subj.lower():
        hdrs, bdy = with_pdf(headers, body, subj.split()[0].lower() + ".pdf")
    else:
        hdrs, bdy = plain(headers, body)
    n += 1
    write_eml("INBOX", f"{n:03}.eml", hdrs, bdy, date)

# robots: invoices twice a year, newsletters quarterly, spread over a decade
for y in range(2016, 2026):
    for i, (rname, raddr, kind) in enumerate(ROBOTS):
        months = [3, 9] if kind == "invoice" else [1, 4, 7, 10]
        for m in months:
            date = datetime(y, m, 5 + i * 3, 10, tzinfo=timezone.utc)
            subj = (
                f"Rechnung {rname} {y}-{m:02}" if kind == "invoice"
                else f"Newsletter {rname} {y}-{m:02}"
            )
            headers = base_headers(f"rob{y}{m}{i}@demo.example", subj, (rname, raddr), ME, date)
            if kind == "invoice":
                hdrs, bdy = with_pdf(
                    headers,
                    f"Guten Tag\n\nIhre Rechnung fuer {y}-{m:02} liegt als PDF bei.\n\n{rname}",
                    f"rechnung-{y}-{m:02}.pdf",
                )
            else:
                hdrs, bdy = plain(headers, f"News and updates from {rname}, edition {y}-{m:02}.")
            n += 1
            # archives get years-old mail; a few keep fresh mtimes -> skew demo
            write_eml("Archives", f"{n:03}.eml", hdrs, bdy, date, mtime_now=(y == 2019 and i == 0 and m == 3))

print(f"{n} messages in {OUT}/")
