document.addEventListener('DOMContentLoaded', () => {
    const contextMenu = document.getElementById('context-menu');
    const fileBrowser = document.getElementById('file-browser');
    let currentContextItem = null; // The li element that was right-clicked

    // --- Show Context Menu ---
    fileBrowser.addEventListener('contextmenu', (event) => {
        const targetLi = event.target.closest('li[data-path]'); // Find the parent li with a path

        if (targetLi) {
            event.preventDefault(); // Prevent default browser menu
            currentContextItem = targetLi; // Store the item

            const path = targetLi.getAttribute('data-path');
            const isDir = targetLi.getAttribute('data-is-dir') === 'true';
            const shareButton = document.getElementById('context-share');
            const shareLinkDisplay = document.getElementById('share-link-display');

            // Reset previous share link display if any
            if (shareLinkDisplay) {
                shareLinkDisplay.remove();
            }

            // Only show share for files for now
            if (!isDir && shareButton) {
                 // Update hx-vals for the share button before showing
                shareButton.setAttribute('hx-vals', `{"path": "${path}"}`);
                shareButton.style.display = 'block'; // Ensure share button is visible
            } else if (shareButton) {
                shareButton.style.display = 'none'; // Hide share for directories
            }


            // Position and show the menu
            contextMenu.style.top = `${event.clientY}px`;
            contextMenu.style.left = `${event.clientX}px`;
            contextMenu.style.display = 'block';
        } else {
             // If right-click wasn't on a file/dir item, hide custom menu
             hideContextMenu();
        }
    });

    // --- Hide Context Menu ---
    function hideContextMenu() {
        if (contextMenu) {
            contextMenu.style.display = 'none';
        }
        currentContextItem = null; // Clear the stored item
    }

    // Hide menu when clicking elsewhere in the document
    document.addEventListener('click', (event) => {
        // If the click is outside the context menu, hide it
        if (contextMenu && !contextMenu.contains(event.target)) {
            hideContextMenu();
        }
    });

     // Hide menu on scroll
    window.addEventListener('scroll', hideContextMenu, true); // Use capture phase


    // --- Handle Share Action (delegated from context menu) ---
    // The actual POST is handled by HTMX on the #context-share button
    // We just need to hide the menu after the action starts or finishes.
    // Option 1: Hide immediately on click (simpler)
     contextMenu.addEventListener('click', (event) => {
         if (event.target.closest('#context-share')) {
             // Let HTMX handle the request, just hide the menu
              // Add a slight delay to allow htmx to potentially swap content first
              // Or rely on hx-swap to replace the button/show link
             setTimeout(hideContextMenu, 50);
         }
     });


    // --- Displaying the Share Link ---
    // HTMX will replace the #context-share-target content.
    // We might need to ensure the context menu stays visible *briefly*
    // or reposition it if the content swap changes layout significantly.
    // For simplicity now, let's assume the swap happens within the menu item.

    // We need to inject the share result *near* the item or replace the button
    htmx.on('htmx:afterSwap', function(evt) {
        // Check if the swap target was our share result container
        if (evt.detail.target.id === 'context-share-target') {
             // The link is now displayed. Keep the context menu open briefly
             // or find a better UX. For now, we do nothing extra,
             // rely on the user copying the link before clicking away.
             const input = evt.detail.target.querySelector('input[type="text"]');
             if (input) {
                input.select(); // Auto-select the link text
             }
        }
    });


    // Close context menu if Escape key is pressed
    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') {
            hideContextMenu();
        }
    });
});
