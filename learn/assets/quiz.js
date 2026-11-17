// Multiple-choice questions with immediate feedback.
//
// Authoring:
//
//   <div class="quiz">
//     <p class="q">Question text</p>
//     (optional code block, see code.js)
//     <ul class="options">            add data-fixed to keep the written order
//       <li>Wrong answer</li>
//       <li data-correct>Right answer</li>
//     </ul>
//     <div class="explain">Shown once the right answer is picked.</div>
//   </div>
//
// Options are shuffled on every visit, so position can't be memorised. A wrong pick is struck through and the
// learner tries again; the explanation appears once they find the right answer. When every question on the page
// is solved, the lesson is marked done (see course.js), and an element with [data-score] shows the first-try score.

(function () {
  "use strict";

  function shuffleChildren(ul) {
    const items = Array.from(ul.children);
    for (let i = items.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [items[i], items[j]] = [items[j], items[i]];
    }
    items.forEach((li) => ul.appendChild(li));
  }

  const ENCOURAGE = ["Not quite. Try another.", "Close, but no. Pick again.", "Not this one. Try again."];

  document.addEventListener("DOMContentLoaded", () => {
    const quizzes = Array.from(document.querySelectorAll(".quiz"));
    let firstTry = 0;
    let solved = 0;

    const updateScore = () => {
      document.querySelectorAll("[data-score]").forEach((el) => {
        el.textContent = solved === quizzes.length
          ? "All " + quizzes.length + " solved, " + firstTry + " on the first try."
          : solved + " of " + quizzes.length + " solved.";
      });
    };

    quizzes.forEach((quiz, index) => {
      const q = quiz.querySelector(".q");
      if (q) q.dataset.n = "Question " + (index + 1) + " of " + quizzes.length;
      const ul = quiz.querySelector("ul.options");
      if (!ul) return;
      if (!ul.hasAttribute("data-fixed")) shuffleChildren(ul);
      const feedback = document.createElement("div");
      feedback.className = "feedback";
      feedback.setAttribute("aria-live", "polite");
      ul.after(feedback);
      let misses = 0;

      ul.querySelectorAll("li").forEach((li) => {
        li.tabIndex = 0;
        const choose = () => {
          if (quiz.classList.contains("solved") || li.classList.contains("wrong")) return;
          if (li.hasAttribute("data-correct")) {
            li.classList.add("right");
            quiz.classList.add("solved");
            feedback.className = "feedback good";
            feedback.textContent = misses === 0 ? "Correct, first try." : "Correct.";
            if (misses === 0) firstTry++;
            solved++;
            updateScore();
            if (solved === quizzes.length && window.LedgerlyCourse) window.LedgerlyCourse.markDone();
          } else {
            li.classList.add("wrong");
            feedback.className = "feedback bad";
            feedback.textContent = ENCOURAGE[misses % ENCOURAGE.length];
            misses++;
          }
        };
        li.addEventListener("click", choose);
        li.addEventListener("keydown", (e) => {
          if (e.key === "Enter" || e.key === " ") { e.preventDefault(); choose(); }
        });
      });
    });
    updateScore();
  });

})();
