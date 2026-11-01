import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.regex.Pattern;

import robocode.BattleResults;
import robocode.control.BattleSpecification;
import robocode.control.BattlefieldSpecification;
import robocode.control.RobocodeEngine;
import robocode.control.RobotSpecification;
import robocode.control.events.BattleAdaptor;
import robocode.control.events.BattleCompletedEvent;
import robocode.control.events.BattleErrorEvent;
import robocode.control.events.TurnEndedEvent;
import robocode.control.snapshot.IRobotSnapshot;

/**
 * Runs one robot against fixed opponents with enemy names hidden and shown, and reports its score and
 * any errors it printed, so a robot that breaks on hidden names shows up.
 *
 * usage: NameCheck HOME ROUNDS WIDTH HEIGHT ROBOT OPPONENTS_CSV OUTPUT_FILE
 *
 * One line per battle, tab separated: robot, mode, opponents, score share, firsts, errors, first error.
 */
public class NameCheck {
	static final Pattern ERROR = Pattern.compile("Exception|\\bError\\b|\\berror:|^\\s+at [\\w.$]+\\(|SYSTEM: .*(stopped|not allowed|disabled|punish)|StackOverflow|OutOfMemory");
	static int errors;
	static String firstError;
	static BattleResults[] results;

	public static void main(String[] args) throws Exception {
		File home = new File(args[0]);
		int rounds = Integer.parseInt(args[1]);
		BattlefieldSpecification field = new BattlefieldSpecification(Integer.parseInt(args[2]), Integer.parseInt(args[3]));
		String robot = args[4];
		String opponents = args[5];
		String robots = robot + "," + opponents;

		// As in the harness: load these in this trusted thread, not first inside a robot's sandbox.
		java.time.ZonedDateTime.now().format(java.time.format.DateTimeFormatter.ISO_ZONED_DATE_TIME);
		String.format(Locale.getDefault(), "%.3f %d %s", 1.5, 2, new java.util.Date());

		RobocodeEngine engine = new RobocodeEngine(home);
		engine.setVisible(false);
		engine.addBattleListener(new BattleAdaptor() {
			@Override
			public void onTurnEnded(TurnEndedEvent event) {
				for (IRobotSnapshot snapshot : event.getTurnSnapshot().getRobots()) {
					if (!snapshot.getName().equals(robot)) {
						continue;
					}
					String out = snapshot.getOutputStreamSnapshot();
					if (out == null || out.isEmpty()) {
						continue;
					}
					for (String line : out.split("\n")) {
						if (ERROR.matcher(line).find()) {
							errors++;
							if (firstError == null) {
								firstError = line.trim();
							}
						}
					}
				}
			}

			@Override
			public void onBattleCompleted(BattleCompletedEvent event) {
				results = event.getSortedResults();
			}

			@Override
			public void onBattleError(BattleErrorEvent event) {
				errors++;
				if (firstError == null) {
					firstError = "battle error: " + event.getError();
				}
			}
		});

		RobotSpecification[] specs = engine.getLocalRepository(robots);
		List<String> lines = new ArrayList<>();
		// A random order, so that warming up and saved data do not favor one mode.
		boolean hiddenFirst = new java.util.Random().nextBoolean();
		for (boolean hide : new boolean[] {hiddenFirst, !hiddenFirst}) {
			errors = 0;
			firstError = null;
			results = null;
			String mode = hide ? "hidden" : "shown";
			if (specs.length != robots.split(",").length) {
				lines.add(String.join("\t", robot, mode, opponents, "", "", "1", "could not load the robots: " + specs.length));
				continue;
			}
			engine.runBattle(new BattleSpecification(field, rounds, 450, 0.1, 100, hide, specs), true);
			double own = 0, total = 0;
			int firsts = 0;
			if (results != null) {
				for (BattleResults result : results) {
					total += result.getScore();
					if (result.getTeamLeaderName().equals(robot)) {
						own = result.getScore();
						firsts = result.getFirsts();
					}
				}
			}
			String share = total > 0 ? String.format(Locale.ROOT, "%.1f", 100 * own / total) : "";
			lines.add(String.join("\t", robot, mode, opponents, share, Integer.toString(firsts), Integer.toString(errors),
					firstError == null ? "" : firstError.replace('\t', ' ')));
		}
		try (PrintWriter out = new PrintWriter(new java.io.FileWriter(args[6], true))) {
			for (String line : lines) {
				out.println(line);
			}
		}
		engine.close();
		System.exit(0);
	}
}
