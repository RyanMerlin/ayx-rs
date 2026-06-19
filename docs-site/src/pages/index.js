import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import styles from './index.module.css';

function Feature({ title, description, to, label }) {
  return (
    <Link className={styles.featureCard} to={to}>
      <span className={styles.featureLabel}>{label}</span>
      <Heading as="h3">{title}</Heading>
      <p>{description}</p>
    </Link>
  );
}

export default function Home() {
  return (
    <Layout
      title="AYX-RS Docs"
      description="Documentation for ayx-rs, the Alteryx operator CLI — command reference, configuration, and release notes."
    >
      <main className={styles.pageShell}>
        <section className={styles.hero}>
          <div className={styles.heroCopy}>
            <span className={styles.kicker}>Alteryx operator CLI</span>
            <Heading as="h1" className={styles.heroTitle}>
              Install, configure, and automate Alteryx with ayx-rs.
            </Heading>
            <p className={styles.heroText}>
              Documentation for the <code>ayx</code> CLI — generated command reference,
              configuration contracts, Alteryx Server API, and versioned release notes.
              Every page tracks the live binary surface.
            </p>
            <div className={styles.heroActions}>
              <Link className="button button--primary button--lg" to="/getting-started">
                Get started
              </Link>
              <Link className="button button--secondary button--lg" to="/reference/command-surface">
                Browse commands
              </Link>
            </div>
          </div>
          <div className={styles.heroPanel}>
            <div className={styles.panelHeading}>Current release</div>
            <div className={styles.panelValue}>v0.9.10</div>
            <div className={styles.panelMeta}>Apache-2.0 · Signed + attested · macOS &amp; Linux &amp; Windows</div>
            <div className={styles.panelRule} />
            <div className={styles.panelStatRow}>
              <div>
                <div className={styles.panelStatLabel}>Commands</div>
                <div className={styles.panelStatValue}>180+</div>
              </div>
              <div>
                <div className={styles.panelStatLabel}>Hosted on</div>
                <div className={styles.panelStatValue}>CF Pages</div>
              </div>
            </div>
          </div>
        </section>

        <section className={styles.features}>
          <Feature
            label="Start here"
            title="Install and onboard"
            description="Get from zero to a working profile and first command in minutes using the platform install scripts."
            to="/getting-started"
          />
          <Feature
            label="Reference"
            title="Command surface"
            description="180+ commands annotated by safety posture: read-only vs. mutating, --apply gate, and audit artifacts."
            to="/reference/command-surface"
          />
          <Feature
            label="Safety"
            title="Safety model"
            description="Read-only commands need no flags. Mutating commands require --apply. Dry-run by default."
            to="/safety-model"
          />
          <Feature
            label="API"
            title="Alteryx Server API"
            description="Browse the Alteryx Server V3 REST API — paths, parameters, and response shapes rendered from the spec."
            to="/reference/api/"
          />
        </section>

        <section className={styles.noteBlock}>
          <Heading as="h2">Documentation that follows the binary</Heading>
          <p>
            The command surface is regenerated from the live <code>clap</code> tree on every CI run.
            Release notes are published with each tag. If the site and your binary disagree,
            check <code>ayx --version</code> against the{' '}
            <Link to="/releases">release notes</Link> for your version.
          </p>
        </section>
      </main>
    </Layout>
  );
}
