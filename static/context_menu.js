document.addEventListener('DOMContentLoaded', () => {
    const contextMenu = document.getElementById('context-menu');
    const fileBrowser = document.getElementById('file-browser');
    let currentContextItem = null;
    // --- Variable to store coordinates of the share click ---
    let lastShareClickCoords = null;

    // --- Show Context Menu ---
    fileBrowser.addEventListener('contextmenu', (event) => {
        const targetLi = event.target.closest('li[data-path]');
        if (targetLi) {
            event.preventDefault();
            currentContextItem = targetLi;

            const path = targetLi.getAttribute('data-path');
            const isDir = targetLi.getAttribute('data-is-dir') === 'true';
            const shareButton = document.getElementById('context-share');

            // Remove any existing popups when showing the context menu (optional cleanup)
            // document.querySelectorAll('.share-link-popup').forEach(popup => popup.remove());

            if (!isDir && shareButton) {
                shareButton.setAttribute('hx-vals', `{"path": "${path}"}`);
                shareButton.style.display = 'block';
                // Ensure the target inside the context menu is visible/reset
                const shareTargetLi = document.getElementById('context-share-target');
                if (shareTargetLi) {
                    // Reset its content back to the button if it was replaced previously
                    // (This might not be strictly needed if we always clear it in the response)
                    shareTargetLi.innerHTML = `<button id="context-share" hx-post="/share" hx-trigger="click" hx-target="#context-share-target" hx-swap="innerHTML" hx-vals='{"path": "${path}"}'>🔗 Share File</button>`;
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
        currentContextItem = null;
        // Clear coords when menu is hidden generally
        // lastShareClickCoords = null; // --> Don't clear here, need it for afterSwap
    }

    // Hide menu on click elsewhere or scroll
    document.addEventListener('click', (event) => {
        if (contextMenu && !contextMenu.contains(event.target) && !event.target.closest('.share-link-popup')) {
            // Also don't hide if clicking inside the new popup
            hideContextMenu();
        }
        // If clicking outside the popup *after* it appeared, maybe hide the popup too?
        // else if (!event.target.closest('.share-link-popup') && document.querySelector('.share-link-popup')) {
        //      document.querySelectorAll('.share-link-popup').forEach(popup => popup.remove());
        // }
    });
    window.addEventListener('scroll', hideContextMenu, true);
    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') {
            hideContextMenu();
            // Also hide popups on Escape
            document.querySelectorAll('.share-link-popup').forEach(popup => popup.remove());
        }
    });


    // --- Store Coordinates on Share Click & Hide Menu ---
    contextMenu.addEventListener('click', (event) => {
        // Use closest() to ensure we catch clicks even on icons/spans inside the button
        const shareButtonClicked = event.target.closest('#context-share');
        if (shareButtonClicked) {
            // Store the coordinates where the share button was clicked
            lastShareClickCoords = { x: event.clientX, y: event.clientY };
            // Let HTMX handle the POST, just hide the context menu after a short delay
            setTimeout(hideContextMenu, 100); // Increased delay slightly
        }
    });

    // --- Position Popup After HTMX Swap ---
    htmx.on('htmx:afterSwap', function(evt) {
        // Check if a popup was potentially added by looking for the class
        // Since we append to body, the swapped element might not be the popup itself.
        // We need to find the *newly added* element. A common pattern is to look
        // for the element by ID if the server included a unique ID.

        // Let's try finding the *last* element with the popup class added to the body
        const addedPopups = document.querySelectorAll('body > .share-link-popup:last-of-type');

        if (addedPopups.length > 0 && lastShareClickCoords) {
            const newPopup = addedPopups[addedPopups.length - 1]; // Get the most recently added one

            // Adjust coordinates slightly if needed (e.g., offset from cursor)
            const offsetX = 10;
            const offsetY = 10;

            // Position the popup near the stored coordinates
            newPopup.style.left = `${lastShareClickCoords.x + offsetX}px`;
            newPopup.style.top = `${lastShareClickCoords.y + offsetY}px`;
            newPopup.style.visibility = 'visible'; // Make it visible

            // Clear coords after use to prevent reusing for unrelated swaps
            lastShareClickCoords = null;

            // Auto-select input text within *this* specific popup
            const input = newPopup.querySelector('input[type="text"]');
            if (input) {
                input.select();
            }
        }
        // No need to check evt.detail.elt specifically if we search the body
    });

    // Make sure the copy functionality still works (copy_link.js handles this)
});
