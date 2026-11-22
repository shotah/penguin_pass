(function () {
  "use strict";

  const SHOTS = [
    {
      src: "assets/alpine-peloton-1080.png",
      label: "Alpine Peloton & Drafting",
      tab: "Peloton",
    },
    {
      src: "assets/desert-volcano-1080.png",
      label: "Red Rock Gorge & Desert Climbs",
      tab: "Desert",
    },
    {
      src: "assets/workout-1080.png",
      label: "Structured ERG Workouts",
      tab: "Workouts",
    },
    {
      src: "assets/progress-1080.png",
      label: "Progress, Kits & Personal Records",
      tab: "Progress",
    },
    {
      src: "assets/title-1080.png",
      label: "Course & Workout Picker",
      tab: "Menus",
    },
  ];

  const ATMOSPHERE = [
    { src: "assets/alpine-dawn.png", label: "Alpine dawn" },
    { src: "assets/alpine-sunset.png", label: "Alpine sunset" },
    { src: "assets/alpine-night.png", label: "Alpine night" },
    { src: "assets/alpine-rain.png", label: "Alpine rain" },
    { src: "assets/alpine-snow.png", label: "Alpine snow" },
  ];

  let activeIndex = 0;

  const tabsEl = document.querySelector("[data-gallery-tabs]");
  const thumbsEl = document.querySelector("[data-gallery-thumbs]");
  const mainImg = document.querySelector("[data-gallery-image]");
  const mainCap = document.querySelector("[data-gallery-caption]");
  const atmosphereEl = document.querySelector("[data-atmosphere]");
  const lightbox = document.getElementById("lightbox");
  const lightboxImg = document.querySelector("[data-lightbox-img]");
  const lightboxCap = document.querySelector("[data-lightbox-cap]");

  function setShot(index) {
    activeIndex = index;
    const shot = SHOTS[index];
    mainImg.src = shot.src;
    mainImg.alt = shot.label;
    mainCap.textContent = shot.label;

    tabsEl.querySelectorAll(".gallery-tab").forEach((btn, i) => {
      btn.classList.toggle("is-active", i === index);
      btn.setAttribute("aria-selected", i === index ? "true" : "false");
    });
    thumbsEl.querySelectorAll(".gallery-thumb").forEach((btn, i) => {
      btn.classList.toggle("is-active", i === index);
    });
  }

  function openLightbox(src, label) {
    lightboxImg.src = src;
    lightboxImg.alt = label;
    lightboxCap.textContent = label;
    lightbox.hidden = false;
    lightbox.classList.add("is-open");
    document.body.style.overflow = "hidden";
  }

  function closeLightbox() {
    lightbox.classList.remove("is-open");
    lightbox.hidden = true;
    lightboxImg.src = "";
    document.body.style.overflow = "";
  }

  SHOTS.forEach((shot, i) => {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "gallery-tab" + (i === 0 ? " is-active" : "");
    tab.textContent = shot.tab;
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-selected", i === 0 ? "true" : "false");
    tab.addEventListener("click", () => setShot(i));
    tabsEl.appendChild(tab);

    const thumb = document.createElement("button");
    thumb.type = "button";
    thumb.className = "gallery-thumb" + (i === 0 ? " is-active" : "");
    const img = document.createElement("img");
    img.src = shot.src;
    img.alt = "";
    img.loading = "lazy";
    img.width = 200;
    img.height = 112;
    thumb.appendChild(img);
    thumb.addEventListener("click", () => setShot(i));
    thumbsEl.appendChild(thumb);
  });

  ATMOSPHERE.forEach((item) => {
    const btn = document.createElement("button");
    btn.type = "button";
    const img = document.createElement("img");
    img.src = item.src;
    img.alt = item.label;
    img.loading = "lazy";
    btn.appendChild(img);
    btn.addEventListener("click", () => openLightbox(item.src, item.label));
    atmosphereEl.appendChild(btn);
  });

  mainImg.addEventListener("click", () => {
    openLightbox(SHOTS[activeIndex].src, SHOTS[activeIndex].label);
  });

  document.querySelector("[data-lightbox-close]")?.addEventListener("click", closeLightbox);
  lightbox.addEventListener("click", (e) => {
    if (e.target === lightbox) closeLightbox();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && lightbox.classList.contains("is-open")) closeLightbox();
  });

  const menuToggle = document.querySelector("[data-menu-toggle]");
  const navMobile = document.getElementById("nav-mobile");
  menuToggle?.addEventListener("click", () => {
    const open = navMobile.classList.toggle("is-open");
    navMobile.hidden = !open;
    menuToggle.setAttribute("aria-expanded", open ? "true" : "false");
  });
  navMobile?.querySelectorAll("a").forEach((a) => {
    a.addEventListener("click", () => {
      navMobile.classList.remove("is-open");
      navMobile.hidden = true;
      menuToggle?.setAttribute("aria-expanded", "false");
    });
  });

  document.querySelectorAll("[data-copy-target]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.getAttribute("data-copy-target");
      const pre = document.getElementById(id);
      if (!pre) return;
      const text = pre.textContent.replace(/^\s*Copy\s*/, "").trim();
      try {
        await navigator.clipboard.writeText(text);
        btn.textContent = "Copied!";
        btn.classList.add("copied");
        setTimeout(() => {
          btn.textContent = "Copy";
          btn.classList.remove("copied");
        }, 2000);
      } catch {
        btn.textContent = "Select & copy";
      }
    });
  });
})();