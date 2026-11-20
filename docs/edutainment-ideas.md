# Edutainment ideas

Ideas for educational software for Sekka, collected on 2026-10-31. Nothing here
is planned or committed to. Any of these would start outside this repository,
as Uta does, built against aizome-kit and Sukumo, and would come into the
catalog only once it is worth shipping. Child safety and parental controls are
planned for Sekka, and several ideas below lean on them.

Linux already has GCompris, Tux Paint and KDE Edu, which are mostly suites of
drills. The ideas here aim for what those don't do.

## What could set Sekka's apart

- **Success happens away from the screen.** Many of these could end in
  something you print and build: a sundial cut for your latitude, a zoetrope
  strip, a paper computer. For decades, Gakken's *Kagaku* magazine shipped
  Japanese children a working experiment kit every month as its *furoku*
  (supplement). One project a month is a good cadence because it can't be
  binged.
- **The child makes something.** Every session leaves something they made.
  Parents get a portfolio to look at instead of an activity log.
- **No engagement tricks.** No streaks, guilt-tripping mascots, in-app
  currencies, accounts or network access. That is most of the parental-controls
  pitch already.
- **Use what only Sekka owns:** the compositor, the toolkit and Nix.

## Only Sekka could do these

- **A wind-down signal.** When parental controls arrive, the time-limit service
  could tell apps "ten minutes left" through aizome-kit. Apps then head for a
  natural stopping point (finish the circuit, save the drawing, the pond goes
  dark) instead of being killed mid-thought. Out-of-tree apps like Uta already
  build against the kit, so it would reach them too. It is worth designing
  alongside the first edutainment app, so that app is its first user.
- **Lift the lid.** An x-ray mode over the live desktop. Press a key and watch
  the event travel from the keyboard to Aizome to the focused window. Peel
  windows apart into surfaces, pause frames into a flipbook, see repaint
  regions flash. Mozilla's X-Ray Goggles did a little of this for web pages.
  Only someone who wrote the compositor can do it for a whole desktop.
- **The computer you can't break.** Give a child root in a disposable VM or a
  generation of their own. "Delete something important, watch it fail, roll it
  back" teaches more about computers than any lesson, and NixOS makes the
  rollback real.
- **Two mice, one screen.** Couch co-op for a parent and child, each with their
  own pointer. Whether Aizome's seat handling could take a second pointer is an
  open question. If it could, almost no desktop offers this.
- **The fridge door.** Sharing that stays in the family: what a child makes is
  pinned to a board the household's accounts can see, and nothing leaves the
  house.
- **A creature on the desktop**, in the line of Neko and eSheep. It walks along
  title bars and falls when you drag its window out from under it. Aizome knows
  every window's position.

## Conservative, done well

- A typing tutor tied to Aizome's per-keyboard setup, including kana input
  through IBus.
- A flash-anzan (mental abacus) trainer as a mode of Soroban.
- A successor to Kid Pix: painting with sound effects, a dynamite eraser and
  the Undo Guy. Tux Paint exists but lacks the mischief.
- A reading buddy that listens to a child read aloud and helps when they
  stumble, if Koe's recognition runs on the machine.
- Kamishibai: children make picture-card stories and perform them full screen
  for the family.

## Forgotten software worth reviving

- **Rocky's Boots (1982) and Robot Odyssey (1984).** First you walk around
  inside logic gates, then you wire up robots yourself. Robot Odyssey was
  famously too hard for its audience. Zachtronics later found that audience
  among adults, but nothing good exists for children now.
- **ToonTalk (Ken Kahn, 1995).** You program by showing robots what to do, and
  birds carry messages to their nests. It is concurrent, actor-style
  programming in a cartoon world, and almost nobody remembers it.
- **KidSim, later Stagecast Creator (Apple).** Programming with before-and-after
  picture rules. Baba Is You and PuzzleScript prove the idea is fun.
- **HyperCard for children.** Stacks of cards with buttons. Children haven't had
  an authoring tool like it since.
- **El-Fish (Maxis, 1993), Dawkins's Biomorphs and Creatures (1996).** Breeding
  and evolution you can watch.
- **CARDIAC (Bell Labs, 1968)**, a cardboard computer given to high-school
  students, and the **WDR Papiercomputer (1983)**. Run one on screen, then
  print it out.

## Older than computers

- **Fröbel's Gifts.** The original kindergarten (1830s–40s): blocks, sticks,
  rings, folding and weaving in a strict sequence. Frank Lloyd Wright credited
  them. They reached Japan in 1876 as 恩物 *onbutsu*.
- **Gattegno's *Words in Color* (1962).** Every sound in English gets a colour,
  so a child can *see* that "ough" stands for several different sounds. A
  screen suits it better than the original wall charts.
- **Comenius's *Orbis Pictus* (1658).** The first picture book for children:
  numbered scenes labelled in two languages. It would make a bilingual picture
  encyclopedia to explore.
- **Rithmomachia.** The "philosophers' game", a battle of numbers that monks and
  scholars played from about the 11th to the 17th century to learn Boethius's
  theory of ratios and progressions. It is almost completely lost.
- **Lewis Carroll's *The Game of Logic* (1886).** Syllogisms on a board with
  coloured counters, written for children. Carroll diagrams survive in British
  primary schools; the game doesn't.
- **Chaucer's *Treatise on the Astrolabe* (c. 1391)**, written for his
  ten-year-old son. Make a working astrolabe for your coordinates and use the
  treatise as the tutorial.
- **Sumerian school.** Exercise tablets survive with the teacher's model on one
  side and the student's copy on the other, mistakes included. Do the world's
  oldest homework in base 60 and learn why an hour has 60 minutes. Problem 79
  of the Rhind papyrus (houses, cats, mice, grain) is an ancestor of "As I was
  going to St Ives".
- **Philosophical toys.** The Victorians' own name for the thaumatrope,
  phenakistoscope, zoetrope and harmonograph. Design one, watch it on screen,
  then print and spin it.
- **Llull's wheels and volvelles.** Rotating paper discs that combine ideas;
  Leibniz took them seriously. Children build paper calculating machines.
- **The Landlord's Game (Lizzie Magie, 1904).** Monopoly's ancestor, made to
  show how rules decide who gets rich, and later published with a second,
  cooperative rule set. Play both.
- **MONIAC (1949).** An economy built from water flowing through tanks and
  valves. Watching the money flow is the whole lesson.
- **Change ringing.** English bell-ringers have been working through
  permutation groups by ear since Stedman's *Tintinnalogia* (1668).
- **Heraldry.** "Azure, a bend Or" is a sentence in a formal language. Write a
  blazon and get a shield, or paint a shield and get its blazon. It is a
  compiler disguised as knights.
- **The progymnasmata.** Fourteen graded writing exercises taught from
  Hellenistic schools to Shakespeare's grammar school. They start with
  retelling a fable and end with arguing for or against a law.
- **Sand drawings:** kolam in Tamil Nadu, sona among the Chokwe of Angola, and
  Vanuatu's sand drawings. One continuous line loops around a grid of dots,
  which is Euler paths long before Euler. Kolam are drawn at the doorstep, the
  genkan.

## Japanese deep cuts that suit the naming

- **Sangaku.** Edo-period wooden tablets of geometry problems, hung at shrines
  and temples as offerings. Solve them, then hang your own at a family shrine.
- **Jinkōki (1627).** Edo Japan's best-selling arithmetic book, full of
  puzzles: rats that multiply (nezumi-zan), splitting oil with odd-sized jugs
  (abura-wake), and the stepchildren circle (mamako-date, a Josephus problem).
  It is practically a ready-made list of levels.
- **Genji-kō, played with sound.** In this incense game, guests smell five
  samples and draw which ones matched. There are exactly 52 possible patterns
  (the Bell number B₅), each named after a chapter of *The Tale of Genji*. Play
  it with tones instead of incense and children learn set partitions without
  ever hearing the term.
- **Karakuri.** The *Karakuri Zui* (1796) gave diagrams for mechanical dolls,
  including the tea-serving doll. Build automata from cams, gears and strings.
- **Terakoya.** The Edo temple schools credited with Japan's high literacy
  among ordinary people. It would be a good name for the children's side of
  Sekka as a whole.

## Wacky and radical

- **Pictures from space.** A cheap RTL-SDR stick and a homemade antenna can pick
  up weather-satellite images, planes overhead (ADS-B) and the ISS's SSTV
  broadcasts. It only receives, so it is safe, and it feels like magic.
- **A pond in a jar.** A sealed ecosystem that lives on the machine in real time
  for months. It changes slowly, can't be binged, and doesn't die because the
  family went on holiday.
- **Grow a language.** Invent words and a script, then run centuries of sound
  change and watch the language split into daughter languages.
- **String figures (ayatori, cat's cradle).** Anthropologists recorded hundreds
  from cultures that have since lost them. As far as we know, nobody has built
  a convincing string-figure simulator; it needs both cloth physics and knot
  topology.
- **A process zoo.** psDoom (around 2000) let you shoot your processes. Turn it
  around: Myaku becomes an aquarium where your own programs swim, bigger with
  memory and faster with CPU.
- **Draw sound.** Xenakis's UPIC (1977) let people draw music, and its workshops
  later included children. Daphne Oram and Norman McLaren drew sound directly
  onto film.
- **A letter to the future.** A time capsule the OS keeps sealed for years,
  which a parent can co-sign.
- **Fuller's World Game.** A cooperative 1960s simulation of the world's
  resources, played on a huge Dymaxion map.
- **A teachable agent.** In Vanderbilt's *Betty's Brain*, children taught a
  character by building a concept map, and the character's quiz score counted
  as theirs. Children work harder when they're teaching someone (the protégé
  effect), and it needs no LLM.

## Where to start

1. **Furoku:** a monthly print-and-make app. Early projects could be a sundial
   and star chart for your coordinates, a phenakistoscope, CARDIAC and kolam
   patterns. It grows one project at a time and exercises printing and Shiori.
2. **A successor to Rocky's Boots and Robot Odyssey.** It has the most depth,
   and it would push Sukumo toward a serious interactive canvas.
3. **Genji-kō or sangaku:** something small and polished that is unmistakably
   Sekka's and sets the tone.

The wind-down signal would be designed alongside whichever comes first.

## Open questions

- **What ages?** 4–7, 8–12 and teenagers are almost three different products.
  Who can playtest?
- **AI:** where should Sekka stand on AI in children's software? Options range
  from none at all, to a local model only, to the deliberately ignorant agent a
  child has to teach.
- **What comes first:** making things, puzzles, simulations, or culture and
  history?
