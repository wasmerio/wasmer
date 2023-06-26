describe('wasmer.sh', () => {
  it('can open the web page', () => {
    cy.visit("/")
  })

  it('loads and prints the initial prompt', () => {
    cy.visit('/');

    // Wait for the console to load
    cy.wait(5000);

    cy.get(".xterm-text-layer").screenshot();

    cy.get('body').scrollIntoView().screenshot().matchImage();
  })
})
