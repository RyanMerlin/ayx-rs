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
      description="Versioned documentation for ayx-rs, the Alteryx operator CLI."
    >
      <main className={styles.pageShell}>
        <section className={styles.hero}>
          <div className={styles.heroCopy}>
            <span className={styles.kicker}>CLI docs with release memory</span>
            <Heading as="h1" className={styles.heroTitle}>
              Everything you need to run, debug, and upgrade ayx-rs.
            </Heading>
            <p className={styles.heroText}>
              This docs surface tracks the live command tree, stable release notes,
              and configuration contracts so users on older binaries can still find
              the exact behavior they shipped with.
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
            <div className={styles.panelHeading}>Published on</div>
            <div className={styles.panelValue}>Cloudflare Pages</div>
            <div className={styles.panelMeta}>Versioned docs. Preview builds. Static delivery.</div>
            <div className={styles.panelRule} />
            <div className={styles.panelStatRow}>
              <div>
                <div className={styles.panelStatLabel}>Latest</div>
                <div className={styles.panelStatValue}>main</div>
              </div>
              <div>
                <div className={styles.panelStatLabel}>Release snapshot</div>
                <div className={styles.panelStatValue}>v0.9.10</div>
              </div>
            </div>
          </div>
        </section>

        <section className={styles.features}>
          <Feature
            label="Start here"
            title="Install and onboard"
            description="Get from first clone to a working profile, central config, and a useful first command."
            to="/getting-started"
          />
          <Feature
            label="Reference"
            title="Command surface"
            description="See the generated command inventory, safety posture, and what is mutating versus read-only."
            to="/reference/command-surface"
          />
          <Feature
            label="Upgrades"
            title="Release notes"
            description="Track what changed in each release and what to expect when you move between versions."
            to="/releases"
          />
        </section>

        <section className={styles.noteBlock}>
          <Heading as="h2">Built for versioned CLI documentation</Heading>
          <p>
            This site is intentionally release-aware. Each tagged release can be
            frozen into a browsable docs version, while the current docs keep
            following the latest command surface and config contract.
          </p>
        </section>
      </main>
    </Layout>
  );
}
