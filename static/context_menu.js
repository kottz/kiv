document.addEventListener('DOMContentLoaded', () => {
    const contextMenu = document.getElementById('context-menu');
    const fileBrowser = document.getElementById('file-browser');
    // No longer need currentContextItem or lastShareClickCoords for positioning

    // --- Show Context Menu ---
    fileBrowser.addEventListener('contextmenu', (event) => {
        const targetLi = event.target.closest('li[data-path][id]'); // Ensure it has an ID now
        if (targetLi) {
            event.preventDefault();
            // currentContextItem = targetLi; // Don't strictly need to store this anymore

            const path = targetLi.getAttribute('data-path');
            const isDir = targetLi.getAttribute('data-is-dir') === 'true';
            const shareButton = document.getElementById('context-share');

            // --- Clear any previous share links when showing menu ---
            // Find *all* placeholders and clear them
            document.querySelectorAll('.share-link-placeholder').forEach(ph => {
                ph.innerHTML = '';
            });

            if (!isDir && shareButton) {
                // Reset the button state in the context menu
                const shareTargetLi = document.getElementById('context-share-target');
                if (shareTargetLi) {
                    shareTargetLi.innerHTML = `<button id="context-share" hx-post="/share" hx-trigger="click" hx-target="#context-share-target" hx-swap="innerHTML" hx-vals='{"path": "${path}"}'>🔗 Share File</button>`;
                }
                // Get the potentially recreated button and ensure hx-vals is set
                const newShareButton = document.getElementById('context-share');
                if (newShareButton) {
                    newShareButton.setAttribute('hx-vals', `{"path": "${path}"}`);
                    newShareButton.style.display = 'block';
                }

            } else if (shareButton) {
                shareButton.style.display = 'none';
            }

            contextMenu.style.top = `${event.clientY}px`;
            contextMenu.style.left = `${event.clientX}px`;
            contextMenu.style.display = 'block';
        } else {
            hideContextMenu();
        }
    });

    // --- Hide Context Menu ---
    function hideContextMenu() {
        if (contextMenu) {
            contextMenu.style.display = 'none';
        }
        // No need to clear positioning vars
    }

    // Hide menu on click elsewhere, scroll, escape
    document.addEventListener('click', (event) => {
        // Don't hide menu if clicking inside the new inline share box
        if (contextMenu && !contextMenu.contains(event.target) && !event.target.closest('.share-link-inline-box')) {
            hideContextMenu();
        }
        // Clicking outside the share box *could* close it - find the relevant placeholder and clear it? More complex.
        // Let's rely on the 'x' button for now.
    });
    window.addEventListener('scroll', hideContextMenu, true);
    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') {
            hideContextMenu();
            // Also clear all placeholders on Escape
            document.querySelectorAll('.share-link-placeholder').forEach(ph => {
                ph.innerHTML = '';
            });
        }
    });

    // --- Hide Menu Immediately on Share Click ---
    contextMenu.addEventListener('click', (event) => {
        const shareButtonClicked = event.target.closest('#context-share');
        if (shareButtonClicked) {
            // No need to store coords
            setTimeout(hideContextMenu, 50); // Hide quickly
        }
    });

    // --- Optional: Auto-select Text After Swap ---
    htmx.on('htmx:afterSwap', function(evt) {
        // Check if the swapped content contains our input based on the target
        // The target of the OOB swap is the placeholder div
        if (evt.detail.target && evt.detail.target.classList.contains('share-link-placeholder')) {
            const input = evt.detail.target.querySelector('input[type="text"]');
            if (input) {
                input.select();
            }
        }
    });

    // Ensure copy_link.js is still included and working
});
