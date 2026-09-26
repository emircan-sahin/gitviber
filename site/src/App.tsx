import { useEffect } from "react";
import { Faq } from "./components/Faq.tsx";
import { Footer } from "./components/Footer.tsx";
import { Install } from "./components/Install.tsx";
import { Numbers } from "./components/Numbers.tsx";
import { Rest } from "./components/Rest.tsx";
import { Story } from "./components/Story.tsx";
import { TopBar } from "./components/TopBar.tsx";
import { useI18n } from "./i18n/index.tsx";

/** Fades sections in the first time they scroll into view (styles.css .reveal). */
function useReveal() {
  useEffect(() => {
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          entry.target.setAttribute("data-in", "");
          io.unobserve(entry.target);
        }
      },
      { rootMargin: "0px 0px -8% 0px" },
    );
    document.querySelectorAll(".reveal").forEach((el) => io.observe(el));
    return () => io.disconnect();
  }, []);
}

export function App() {
  const { t } = useI18n();
  useReveal();

  return (
    <>
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-lg focus:bg-fg focus:px-3 focus:py-2 focus:text-page"
      >
        {t.nav.skip}
      </a>
      <TopBar />
      <main id="main">
        <Story />
        <Numbers />
        <Rest />
        <Faq />
        <Install />
      </main>
      <Footer />
    </>
  );
}
