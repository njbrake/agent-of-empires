function copyInstall() {
  const cmd = 'curl -fsSL https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh | bash';
  navigator.clipboard.writeText(cmd).then(() => {
    const btn = document.getElementById('copy-btn');
    btn.innerHTML = '<svg class="w-5 h-5 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/></svg>';
    setTimeout(() => {
      btn.innerHTML = '<svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"/></svg>';
    }, 2000);
  });
}

// Fetch GitHub star count
fetch('https://api.github.com/repos/njbrake/agent-of-empires')
  .then(res => res.json())
  .then(data => {
    const count = data.stargazers_count;
    if (count !== undefined) {
      const formatted = count >= 1000 ? (count / 1000).toFixed(1) + 'k' : count;
      document.getElementById('star-count').textContent = formatted;
    }
  })
  .catch(() => {
    document.getElementById('star-count').textContent = '';
  });

// Theme toggle
function initThemeToggle() {
  function updateIcons(theme) {
    document.querySelectorAll('.theme-icon-sun').forEach(function(el) {
      el.classList.toggle('hidden', theme === 'light');
    });
    document.querySelectorAll('.theme-icon-moon').forEach(function(el) {
      el.classList.toggle('hidden', theme === 'dark');
    });
    document.querySelectorAll('.theme-label').forEach(function(el) {
      el.textContent = theme === 'dark' ? 'Light mode' : 'Dark mode';
    });
  }

  var currentTheme = document.documentElement.dataset.theme || 'dark';
  updateIcons(currentTheme);

  document.querySelectorAll('#theme-toggle, #theme-toggle-mobile').forEach(function(btn) {
    btn.addEventListener('click', function() {
      var next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
      document.documentElement.dataset.theme = next;
      localStorage.setItem('theme', next);
      updateIcons(next);
    });
  });
}

initThemeToggle();

// Scroll-triggered animations
document.addEventListener('DOMContentLoaded', () => {
  const observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) {
        entry.target.classList.add('is-visible');
        observer.unobserve(entry.target);
      }
    });
  }, { threshold: 0.1 });

  document.querySelectorAll('.animate-on-scroll').forEach((el) => {
    observer.observe(el);
  });
});
